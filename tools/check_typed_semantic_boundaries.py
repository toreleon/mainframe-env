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


def check_cics_descriptor_entries(root: Path) -> None:
    entries_dir = root / "crates/foundation/mainframe-env-ir/src/cics_descriptor/executable_entries"
    cics_descriptor_entries = "\n".join(
        production(read(root, path.relative_to(root).as_posix()))
        for path in [entries_dir.with_suffix(".rs"), *sorted(entries_dir.glob("*.rs"))]
    )
    for required in [
        "pub const CICS_EXECUTABLE_DESCRIPTORS",
        'namespace: "cics.file"',
        'namespace: "cics.recovery"',
        "operation: CicsPlanOperation::Read",
        "operation: CicsPlanOperation::Rewrite",
        "operation: CicsPlanOperation::Syncpoint",
    ]:
        require(required in cics_descriptor_entries, f"typed CICS descriptor registry omits {required}")


def check(root: Path) -> None:
    compiler_manifest = read(root, "crates/kernel/mainframe-env-compiler/Cargo.toml")
    compiler_dependencies = tomllib.loads(compiler_manifest).get("dependencies", {})
    for forbidden_dependency in [
        "mainframe-env-interpreter",
        "mainframe-env-host-api",
        "mainframe-env-store-api",
        "mainframe-env-cics",
    ]:
        require(
            forbidden_dependency not in compiler_dependencies,
            f"the language frontend must not depend on {forbidden_dependency}",
        )

    ir_dependencies = tomllib.loads(
        read(root, "crates/foundation/mainframe-env-ir/Cargo.toml")
    ).get("dependencies", {})
    for forbidden_dependency in [
        "mainframe-env-compiler",
        "mainframe-env-interpreter",
        "mainframe-env-host-api",
        "mainframe-env-store-api",
        "mainframe-env-cics",
    ]:
        require(
            forbidden_dependency not in ir_dependencies,
            f"the generic IR framework must not depend on {forbidden_dependency}",
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
        "DecimalPlanWireVersion::PolicyV2",
        "DecimalExecutionPolicy",
        "encode_decimal_assignment_plan",
    ]:
        require(required in decimal_hir, f"typed decimal compiler boundary omits {required}")

    cics_hir = production(read(root, "crates/kernel/mainframe-env-compiler/src/hir/cics.rs"))
    for required in [
        'CICS_PLAN_ATTRIBUTE: &str = "cics_plan"',
        "CICS_EXECUTABLE_DESCRIPTORS",
        "cics_executable_descriptor",
        ".runtime_import",
        "encode_cics_effect_plan",
    ]:
        require(required in cics_hir, f"typed CICS compiler boundary omits {required}")
    reject(
        cics_hir,
        ['"cics.file"', '"cics.recovery"', "Effect::DatasetRead", "Effect::DatasetWrite"],
        "typed CICS compiler ownership",
    )

    cics_descriptor_root = production(
        read(root, "crates/foundation/mainframe-env-ir/src/cics_descriptor.rs")
    )
    cics_descriptor_effects = production(
        read(root, "crates/foundation/mainframe-env-ir/src/cics_descriptor/effects.rs")
    )
    for required in [
        "mod executable_registry;",
        "pub use executable_registry::*;",
        "mod executable_entries;",
        "mod effects;",
        "use effects::*;",
        "pub use executable_entries::CICS_EXECUTABLE_DESCRIPTORS;",
        'pub const CICS_RUNTIME_IMPORT: &str = "host.cics"',
    ]:
        require(required in cics_descriptor_root, f"typed CICS descriptor facade omits {required}")
    for required in ["Effect::DatasetRead", "Effect::DatasetWrite", "Effect::Transaction"]:
        require(
            required in cics_descriptor_effects,
            f"typed CICS descriptor effects omit {required}",
        )
    check_cics_descriptor_entries(root)

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
        "decimal_assignment_plan_wire_version",
        "verify_semantic_contracts",
        "operation_identities",
        "CapturedOperandsReceiverLocalV1",
        "CapturedOperandsAtomicV1",
        "validate_declared_slots",
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

    cics_runtime = read(
        root, "crates/kernel/mainframe-env-interpreter/src/machine/typed_cics.rs"
    ).split("#[cfg(test)]\nmod tests", 1)[0]
    typed_execute = between(
        cics_runtime,
        "pub(super) fn execute(\n",
        "fn legacy_condition_policy(\n",
    )
    reject(
        typed_execute,
        [
            "CicsOperation::from_tokens",
            "legacy_arguments",
            "legacy_destination",
            "split_whitespace",
            "reference(",
            "\n    arguments(",
        ],
        "typed CICS runtime",
    )
    cics_runtime_validation = production(
        read(root, "crates/kernel/mainframe-env-interpreter/src/machine/typed_cics/runtime_validation.rs")
    )
    require(
        "fn plan(operation: &Operation)" in cics_runtime,
        "typed CICS plan validation is missing",
    )
    require(
        "fn validate_runtime_plan" in cics_runtime_validation,
        "typed CICS runtime validation is missing",
    )
    validation = (
        cics_runtime.split("fn plan(operation: &Operation)", 1)[1]
        + cics_runtime_validation
    )
    reject(
        validation,
        ["CicsOperation::from_tokens", "legacy_arguments(", "legacy_destination("],
        "typed CICS validation",
    )
    cics_runtime_registry = production(
        read(root, "crates/kernel/mainframe-env-interpreter/src/machine/typed_cics/registry.rs")
    )
    for required in [
        "decode_cics_effect_plan",
        "CICS_EXECUTABLE_DESCRIPTORS",
        "cics_executable_descriptor_for_identity",
        "cics_executable_descriptor",
        ".runtime_import",
    ]:
        require(
            required in cics_runtime or required in cics_runtime_registry,
            f"typed CICS runtime omits {required}",
        )
    reject(
        cics_runtime,
        ['"cics.file"', '"cics.recovery"', "Effect::DatasetRead", "Effect::DatasetWrite"],
        "typed CICS runtime ownership",
    )

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
        and "operations.extend(typed_decimal::operation_identities())" in machine,
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

    stage = production(
        read(root, "crates/contracts/mainframe-env-compiler-api/src/stage.rs")
    )
    for required in [
        "pub struct VerifiedHir",
        "pub struct LoweredMir",
        "pub struct LegalizedMir",
        "pub fn verify(",
        "pub fn lower(self, module: Module) -> LoweredMir",
        "pub fn legalize(",
        "verify_legal(lowered.module, catalog, profile)",
    ]:
        require(required in stage, f"typed proof-stage boundary omits {required}")

    ir_catalog = production(read(root, "crates/foundation/mainframe-env-ir/src/catalog.rs"))
    ir_verify = production(read(root, "crates/foundation/mainframe-env-ir/src/verify.rs"))
    semantic_verify = production(
        read(root, "crates/foundation/mainframe-env-ir/src/semantic_verify.rs")
    )
    require(
        "pub enum OperationSemanticContract" in ir_catalog
        and "CobolLayoutDefinition" in ir_catalog
        and "pub fn cobol_layout_definition_schema()" in ir_catalog
        and "DecimalAssignment(DecimalOperationContract)" in ir_catalog
        and "CicsEffect(CicsOperationContract)" in ir_catalog
        and "verify_semantic_contracts(module, catalog)" in ir_verify,
        "generic IR verification does not invoke registered dialect semantics",
    )
    require(
        "cics_executable_descriptor" in semantic_verify,
        "generic IR semantic verification bypasses the dialect-owned CICS effect registry",
    )
    for required in [
        "decode_decimal_assignment_plan",
        "decode_cics_effect_plan",
        "decimal plan wire version does not match operation major",
        "operation storage declarations differ from plan slots",
        "typed condition branch mask does not match control topology",
    ]:
        require(required in semantic_verify, f"typed semantic verifier omits {required}")

    framework = read(
        root, "crates/tooling/mainframe-env-conformance/src/framework.rs"
    )
    require(
        "fn compiler_and_interpreter_operation_registries_match()" in framework
        and "mainframe_env_compiler::core_mir_catalog()" in framework
        and "mainframe_env_interpreter::supported_operations()" in framework
        and "CICS_EXECUTABLE_DESCRIPTORS" in framework
        and "OperationSemanticContract::CicsEffect" in framework
        and "schema.allowed_effects" in framework
        and "schema.runtime_import" in framework,
        "compiler and runtime executable registries lack a consistency check",
    )
    cics_coordinator = production(
        read(
            root,
            "crates/tooling/mainframe-env-conformance/src/cics_pilot/coordinator.rs",
        )
    )
    for required in [
        "ExecutionCoordinator::durable(",
        ".execute(",
        "LifecycleEventKind::EffectIntent",
        "LifecycleEventKind::EffectResult",
        "EffectState::Completed",
    ]:
        require(
            required in cics_coordinator,
            f"typed CICS product proof bypasses coordinator invariant {required}",
        )
    reject(
        cics_coordinator,
        ["ScopedHostService::invoke", ".invoke("],
        "typed CICS coordinator proof",
    )

    store_model = production(
        read(root, "crates/contracts/mainframe-env-store-api/src/model.rs")
    )
    server_cobol = production(read(root, "crates/apps/mainframe-env-server/src/cobol.rs"))
    server_artifact = production(
        read(root, "crates/apps/mainframe-env-server/src/cobol/artifact.rs")
    )
    server_product = production(read(root, "crates/apps/mainframe-env-server/src/product.rs"))
    require(
        "pub struct ExecutableArtifactMetadata" in store_model
        and "pub executable: Option<ExecutableArtifactMetadata>" in store_model,
        "artifact persistence does not retain versioned executable metadata",
    )
    require(
        "pub(crate) fn admit_executable_artifact" in server_artifact
        and "ValidatedArtifact::read" in server_artifact
        and "admit_executable_artifact(&record)?" in server_product
        and "ReferenceMachine::from_binary(\n                &record.payload" not in server_cobol
        and "ReferenceMachine::from_binary(\n                &record.payload" not in server_product,
        "normal product load or restore bypasses manifest-aware artifact admission",
    )
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

    decimal_adapter = read(
        root, "crates/tooling/mainframe-env-conformance/src/decimal_adapter.rs"
    )
    for required in [
        'LEDGER_FORMULA_CONTRACT: &str = "ledger.formula@1"',
        "verify_legal",
        'OperationIdentity::new(DECIMAL_NAMESPACE, "assign", major)',
        "DecimalExecutionPolicy::decimal18_v1()",
        "DecimalExecutionPolicy::decimal34_v1()",
    ]:
        require(required in decimal_adapter, f"independent decimal adapter omits {required}")
    reject(
        decimal_adapter,
        ["mainframe_env_compiler", "CobolHir", '"cobol.add@1"', '"cobol.compute@1"'],
        "independent decimal adapter",
    )


def main() -> None:
    root = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else Path(__file__).resolve().parents[1]
    check(root)
    print("typed-semantic-boundaries: pass")


if __name__ == "__main__":
    main()
