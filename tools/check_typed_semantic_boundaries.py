#!/usr/bin/env python3
"""Fail when a migrated typed-HIR path regains runtime grammar parsing."""

from __future__ import annotations

import json
import re
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


def skip_non_code(source: str, position: int) -> int:
    if source.startswith("//", position):
        end = source.find("\n", position + 2)
        return len(source) if end < 0 else end
    if source.startswith("/*", position):
        depth = 1
        cursor = position + 2
        while cursor < len(source) and depth:
            if source.startswith("/*", cursor):
                depth += 1
                cursor += 2
            elif source.startswith("*/", cursor):
                depth -= 1
                cursor += 2
            else:
                cursor += 1
        if depth:
            raise BoundaryError("unterminated Rust block comment")
        return cursor
    if source[position] == "r":
        cursor = position + 1
        while cursor < len(source) and source[cursor] == "#":
            cursor += 1
        if cursor < len(source) and source[cursor] == '"':
            delimiter = '"' + source[position + 1 : cursor]
            end = source.find(delimiter, cursor + 1)
            if end < 0:
                raise BoundaryError("unterminated Rust raw string")
            return end + len(delimiter)
    if source[position] == '"':
        cursor = position + 1
        while cursor < len(source):
            if source[cursor] == "\\":
                cursor += 2
            elif source[cursor] == '"':
                return cursor + 1
            else:
                cursor += 1
        raise BoundaryError("unterminated Rust string")
    if source[position] == "'":
        cursor = position + 1
        if cursor < len(source) and source[cursor] == "\\":
            if source.startswith("\\u{", cursor):
                end = source.find("}", cursor + 3)
                cursor = len(source) if end < 0 else end + 1
            elif source.startswith("\\x", cursor):
                cursor += 4
            else:
                cursor += 2
        else:
            cursor += 1
        if cursor < len(source) and source[cursor] == "'":
            return cursor + 1
    return position


def balanced_end(source: str, position: int, opening: str, closing: str) -> int:
    depth = 1
    cursor = position + 1
    while cursor < len(source):
        skipped = skip_non_code(source, cursor)
        if skipped != cursor:
            cursor = skipped
        elif source[cursor] == opening:
            depth += 1
            cursor += 1
        elif source[cursor] == closing:
            depth -= 1
            cursor += 1
            if depth == 0:
                return cursor
        else:
            cursor += 1
    raise BoundaryError(f"unterminated #[cfg(test)] item: {opening}")


def test_item_end(source: str, position: int) -> int:
    cursor = position
    while True:
        while cursor < len(source):
            if source[cursor].isspace():
                cursor += 1
            elif source.startswith(("//", "/*"), cursor):
                cursor = skip_non_code(source, cursor)
            else:
                break
        if not source.startswith("#[", cursor):
            break
        cursor = balanced_end(source, cursor + 1, "[", "]")

    # Items with an initializer can contain a braced expression before their semicolon.
    item = source[cursor:]
    item = re.sub(r"^pub(?:\s*\([^)]*\))?\s+", "", item)
    semicolon_item = bool(re.match(r"(?:use|static|type)\b", item)) or (
        bool(re.match(r"const\b", item))
        and not re.match(r"const\s+(?:(?:unsafe|async)\s+)*fn\b", item)
    )
    body_item = bool(re.match(
        r"(?:(?:const|unsafe|async|default|auto)\s+)*"
        r"(?:fn|mod|struct|enum|union|impl|trait|extern|macro_rules)\b", item
    ))
    while cursor < len(source):
        skipped = skip_non_code(source, cursor)
        if skipped != cursor:
            cursor = skipped
        elif source[cursor] in "([":
            closing = ")" if source[cursor] == "(" else "]"
            cursor = balanced_end(source, cursor, source[cursor], closing)
        elif source[cursor] == "{":
            cursor = balanced_end(source, cursor, "{", "}")
            if not semicolon_item:
                return cursor
        elif source[cursor] == ";":
            return cursor + 1
        elif source[cursor] in ",}" and not (semicolon_item or body_item):
            return cursor + (source[cursor] == ",")
        else:
            cursor += 1
    raise BoundaryError("unterminated #[cfg(test)] item")


def validate_delimiters(source: str) -> None:
    """Reject broken token scopes before a test-only item can mask them."""
    delimiters = []
    cursor = 0
    closing = {")": "(", "]": "[", "}": "{"}
    while cursor < len(source):
        skipped = skip_non_code(source, cursor)
        if skipped != cursor:
            cursor = skipped
            continue
        char = source[cursor]
        if char in "([{":
            delimiters.append((char, cursor))
        elif char in closing:
            require(
                bool(delimiters) and delimiters[-1][0] == closing[char],
                f"unbalanced Rust delimiter {char} at character {cursor}",
            )
            delimiters.pop()
        cursor += 1
    require(not delimiters, f"unterminated Rust delimiter scopes: {delimiters}")


def production(source: str) -> str:
    validate_delimiters(source)
    # Mask only removed items, retaining source coordinates and production literals.
    # Inner attributes apply to their enclosing item, not to following siblings.
    excluded = []
    scopes = []
    attribute_start = None
    item_start = cursor = 0
    while cursor < len(source):
        skipped = skip_non_code(source, cursor)
        if skipped != cursor:
            cursor = skipped
        elif source.startswith(("#[", "#!["), cursor):
            if attribute_start is None:
                attribute_start = cursor
            inner = source.startswith("#![", cursor)
            opening = cursor + (2 if inner else 1)
            end = balanced_end(source, opening, "[", "]")
            attribute = source[opening + 1 : end - 1]
            code = []
            position = 0
            while position < len(attribute):
                skipped = skip_non_code(attribute, position)
                if skipped != position:
                    code.append(" " * (skipped - position))
                    position = skipped
                else:
                    code.append(attribute[position])
                    position += 1
            if re.fullmatch(r"\s*cfg\s*\(\s*test\s*\)\s*", "".join(code)):
                if inner:
                    if scopes:
                        start, opening = scopes.pop()
                        end = balanced_end(source, opening, "{", "}")
                    else:
                        start, end = 0, len(source)
                else:
                    start, end = attribute_start, test_item_end(source, end)
                excluded.append((start, end))
                cursor = item_start = end
                attribute_start = None
            else:
                cursor = end
        elif source[cursor] == "{":
            attribute_start = None
            scopes.append((item_start, cursor))
            cursor += 1
            item_start = cursor
        elif source[cursor] == "}":
            attribute_start = None
            if scopes:
                scopes.pop()
            cursor += 1
            item_start = cursor
        elif source[cursor] == ";":
            attribute_start = None
            cursor += 1
            item_start = cursor
        else:
            if not source[cursor].isspace():
                attribute_start = None
            cursor += 1
    kept = []
    start = 0
    for opening, end in excluded:
        kept.append(source[start:opening])
        kept.append("".join("\n" if char == "\n" else " " for char in source[opening:end]))
        start = end
    kept.append(source[start:])
    return "".join(kept)


def between(source: str, start: str, end: str) -> str:
    require(start in source and end in source, f"typed boundary markers drifted: {start} / {end}")
    return source.split(start, 1)[1].split(end, 1)[0]


def linked_production(parent: Path, module: str, exported: str) -> str:
    """Follow one plain private module and its explicit public re-export."""
    return _linked_production(parent, module, exported)


def linked_module_production(parent: Path, module: str) -> str:
    """Follow one actual plain private module without requiring a public export."""
    return _linked_production(parent, module, None)


def _production_statements(source: str) -> list[str]:
    # Use the same lexer and cfg(test) exclusion as every production check.
    code = list(source)
    cursor = 0
    while cursor < len(source):
        skipped = skip_non_code(source, cursor)
        if skipped != cursor:
            code[cursor:skipped] = ["\n" if char == "\n" else " " for char in source[cursor:skipped]]
            cursor = skipped
        else:
            cursor += 1
    code = "".join(code)
    statements = []
    depth = start = 0
    for position, char in enumerate(code):
        if char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
            if depth == 0:
                start = position + 1
        elif char == ";" and depth == 0:
            statements.append(code[start:position + 1])
            start = position + 1
    return statements


def linked_module_production_if_declared(parent: Path, module: str) -> str:
    """Follow an authenticated file module when declared; keep a flat owner valid."""
    require(
        re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", module) is not None,
        "linked production source expects Rust identifiers",
    )
    source = production(parent.read_text(encoding="utf-8"))
    declarations = [
        item for item in _production_statements(source)
        if re.search(rf"\bmod\s+{module}\s*;\s*$", item)
    ]
    if not declarations:
        return ""
    require(
        len(declarations) == 1
        and re.fullmatch(rf"\s*mod\s+{module}\s*;\s*", declarations[0]) is not None,
        f"production parent requires one plain private module {module}",
    )
    return linked_module_production(parent, module)


def _linked_production(parent: Path, module: str, exported: str | None) -> str:
    names = (module,) if exported is None else (module, exported)
    require(
        all(re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", name) for name in names),
        "linked production source expects Rust identifiers",
    )
    source = production(parent.read_text(encoding="utf-8"))
    statements = _production_statements(source)
    require(
        any(re.fullmatch(rf"\s*mod\s+{module}\s*;\s*", item) for item in statements),
        f"production parent omits plain private module {module}",
    )
    if exported is not None:
        require(
            any(re.fullmatch(rf"\s*pub\s+use\s+{module}\s*::\s*{exported}\s*;\s*", item)
                for item in statements),
            f"production parent omits explicit re-export {module}::{exported}",
        )
    require(parent.suffix == ".rs" and parent.stem != "mod", "unsupported linked parent source")
    child = parent.with_suffix("") / f"{module}.rs"
    return production(child.read_text(encoding="utf-8"))


def reject(source: str, patterns: list[str], scope: str) -> None:
    for pattern in patterns:
        require(pattern not in source, f"{scope} contains forbidden runtime grammar path {pattern}")


def check_cics_descriptor_entries(root: Path) -> None:
    entries_dir = root / "crates/foundation/mainframe-env-ir/src/cics_descriptor/executable_entries"
    cics_descriptor_entries = "\n".join(
        production(read(root, path.relative_to(root).as_posix()))
        for path in [entries_dir.with_suffix(".rs"), *sorted(entries_dir.glob("*.rs"))]
    )
    require(
        re.search(
            r"^pub (?:const|static) CICS_EXECUTABLE_DESCRIPTORS: "
            r"\[CicsExecutableDescriptor; [0-9]+\] = \[",
            cics_descriptor_entries,
            re.MULTILINE,
        )
        is not None,
        "typed CICS descriptor registry omits the pub const or pub static array",
    )
    for required in [
        'namespace: "cics.file"',
        'namespace: "cics.recovery"',
        "operation: CicsPlanOperation::Read",
        "operation: CicsPlanOperation::Rewrite",
        "operation: CicsPlanOperation::Syncpoint",
    ]:
        require(required in cics_descriptor_entries, f"typed CICS descriptor registry omits {required}")


def check_product_artifact_admission(root: Path) -> None:
    store_model = production(
        read(root, "crates/contracts/mainframe-env-store-api/src/model.rs")
    )
    server_cobol = production(read(root, "crates/apps/mainframe-env-server/src/cobol.rs"))
    server_artifact = production(
        read(root, "crates/apps/mainframe-env-server/src/cobol/artifact.rs")
    )
    server_product = production(read(root, "crates/apps/mainframe-env-server/src/product.rs"))
    server_product += "\n" + linked_module_production_if_declared(
        root / "crates/apps/mainframe-env-server/src/product.rs", "online_machine"
    )
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

    cics_runtime = production(read(
        root, "crates/kernel/mainframe-env-interpreter/src/machine/typed_cics.rs"
    ))
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

    check_product_artifact_admission(root)
    versions = json.loads(read(root, "conformance/subsystems/platform/inventory/contracts.json"))
    require(
        versions.get("contracts", {}).get("artifact") == "mainframe-env.artifact@3",
        "the subsystem contract inventory must declare artifact @3",
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
    if sys.argv[1:] == ["--production-module-file"]:
        request = json.load(sys.stdin)
        require(isinstance(request, list) and len(request) == 2
                and all(isinstance(item, str) for item in request),
                "linked production module scanner expects parent and module")
        json.dump([linked_module_production(Path(request[0]), request[1])], sys.stdout)
        return
    if sys.argv[1:] == ["--production-linked-file"]:
        request = json.load(sys.stdin)
        require(isinstance(request, list) and len(request) == 3
                and all(isinstance(item, str) for item in request),
                "linked production scanner expects parent, module and export")
        json.dump([linked_production(Path(request[0]), request[1], request[2])], sys.stdout)
        return
    if sys.argv[1:] == ["--production-files"]:
        paths = json.load(sys.stdin)
        require(
            isinstance(paths, list) and all(isinstance(path, str) for path in paths),
            "production scanner expects an array of file paths",
        )
        json.dump([production(Path(path).read_text(encoding="utf-8")) for path in paths], sys.stdout)
        return
    root = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else Path(__file__).resolve().parents[1]
    check(root)
    print("typed-semantic-boundaries: pass")


if __name__ == "__main__":
    main()
