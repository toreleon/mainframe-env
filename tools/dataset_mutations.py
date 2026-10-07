#!/usr/bin/env python3
"""Candidate-bound behavioral mutation tests of reviewed model/runtime slices.

Mutate transition *source*, never observations, test assertions or live product
state. A compiler failure, missing assertion, or timeout receives no kill credit.
Each receipt distinguishes independent-model and product-runtime credit; no local
mutation grants licensed IBM differential credit.
"""
from __future__ import annotations

import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import re
import subprocess
import tarfile
import tempfile
import time
from dataclasses import dataclass

SOURCE = Path('crates/tooling/mainframe-env-conformance/src/dataset_reference.rs')
CICS_FILE_SOURCE = Path('crates/providers/mainframe-env-cics/src/handlers/file_control.rs')
CICS_RECOVERY_SOURCE = Path('crates/providers/mainframe-env-cics/src/handlers/recovery.rs')
CICS_SOURCES = (CICS_FILE_SOURCE, CICS_RECOVERY_SOURCE)
CICS_SCENARIOS = Path('crates/tooling/mainframe-env-conformance/src/cics_pilot.rs')
COBOL_MOVE_SOURCE = Path('crates/kernel/mainframe-env-interpreter/src/machine.rs')
COBOL_MOVE_SCENARIOS = Path('crates/tooling/mainframe-env-conformance/src/cobol_move_pilot.rs')
TYPED_ARITHMETIC_SOURCE = Path(
    'crates/kernel/mainframe-env-interpreter/src/machine/typed_decimal.rs'
)
COBOL_CORRESPONDING_SOURCE = Path(
    'crates/kernel/mainframe-env-compiler/src/hir/typed.rs'
)
COBOL_CORRESPONDING_SCENARIOS = Path(
    'crates/tooling/mainframe-env-conformance/src/framework.rs'
)
SCHEMA_VERSION = 'mainframe-env.source-mutations@4'
TEST_PREFIX = 'dataset_reference::tests::'
COMMAND = ['cargo', 'test', '--locked', '--offline', '-p', 'mainframe-env-conformance',
           TEST_PREFIX, '--', '--test-threads=1']
ANSI = re.compile(r'\x1b\[[0-9;]*m')
TEST = re.compile(r'^test (\S+) \.\.\. (ok|FAILED)$', re.M)
EXPECTED_TESTS = {
    TEST_PREFIX + 'reference_simulation_covers_five_organizations_and_all_31_commands',
    TEST_PREFIX + 'reference_capability_boundaries_never_become_modeled_success',
    TEST_PREFIX + 'reference_properties_and_perturbations_are_exact_and_repeatable',
}


@dataclass(frozen=True)
class Mutation:
    identity: str
    behavior: str
    old: str
    new: str
    source: Path | None = None


MUTATIONS = (
    Mutation('ksds-descending-order', 'Reverse the KSDS transition sort comparator',
             '.cmp(&right[key_offset..key_offset + key_length])',
             '.cmp(&right[key_offset..key_offset + key_length]).reverse()'),
    Mutation('omit-aix-publication', 'Commit the base record but omit upgraded AIX publication',
             'for (name, index) in updated {\n            self.indexes.insert(name, index);\n        }',
             'drop(updated);'),
    Mutation('publish-failed-transaction', 'Publish transaction state despite an injected failure',
             'if inject_failure {\n            Condition::InjectedFailure',
             'if inject_failure {\n            *self = next;\n            Condition::InjectedFailure'),
    Mutation('omit-gdg-scratch', 'Retain a scratched generation dataset after GDG rollover',
             'self.datasets.remove(&retired);', 'let _ = &retired;'),
)

CICS_COMMAND = [
    'cargo', 'test', '--locked', '--offline', '-p', 'mainframe-env-conformance',
    'cics_pilot::tests::cics_pilot_runs_compiler_interpreter_coordinator_providers_and_real_stores',
    '--', '--exact', '--test-threads=1',
]
CICS_EXPECTED_TESTS = {
    'cics_pilot::tests::cics_pilot_runs_compiler_interpreter_coordinator_providers_and_real_stores'
}
CICS_MUTATIONS = (
    Mutation(
        'cics-omit-rewrite',
        'Report a successful product REWRITE without invoking the dataset transition',
        '    } else {\n        service.nested(run, HostRequest::Dataset(host_request))\n'
        '    }\n    .map_err(|problem| normalize_file_not_found(operation, problem))?;',
        '    } else if operation == CicsOperation::Rewrite {\n'
        '        Ok(HostResult::Dataset(DatasetResult::Mutated { version: 1 }))\n'
        '    } else {\n'
        '        service.nested(run, HostRequest::Dataset(host_request))\n'
        '    }\n    .map_err(|problem| normalize_file_not_found(operation, problem))?;',
        CICS_FILE_SOURCE,
    ),
    Mutation(
        'cics-rollback-noop',
        'Turn product rollback into a durable-undo discard',
        'if outcome == CicsUnitOfWorkOutcome::RolledBack {\n        rollback_run(service, run)?;',
        'if outcome == CicsUnitOfWorkOutcome::RolledBack {\n        service.clear_undo(run)?;',
        CICS_RECOVERY_SOURCE,
    ),
    Mutation(
        'cics-bypass-read-update',
        'Allow a plain READ identity to establish REWRITE context',
        'if update_requested && let Some(identity) = identities.first()',
        'if let Some(identity) = identities.first()',
        CICS_FILE_SOURCE,
    ),
)

COBOL_MOVE_COMMAND = [
    'cargo', 'test', '--locked', '--offline', '-p', 'mainframe-env-conformance',
    'cobol_move_pilot::tests::cobol_numeric_move_runs_source_to_exact_independent_bytes',
    '--', '--exact', '--test-threads=1',
]
COBOL_MOVE_EXPECTED_TESTS = {
    'cobol_move_pilot::tests::cobol_numeric_move_runs_source_to_exact_independent_bytes'
}
COBOL_MOVE_MUTATIONS = (
    Mutation(
        'cobol-reject-edited-truncation',
        'Restore the pre-edit overflow guard so a numeric-edited MOVE errors instead of truncating',
        'if layout.digits > 0\n'
        '        && digits.len() > layout.digits\n'
        '        && layout.category != LayoutCategory::NumericEdited\n'
        '        && !(layout.category == LayoutCategory::Binary && layout.native_binary)\n'
        '    {',
        'if layout.digits > 0 && digits.len() > layout.digits\n'
        '        && !(layout.category == LayoutCategory::Binary && layout.native_binary) {',
    ),
    Mutation(
        'cobol-ignore-floating-capacity',
        'Treat every floating-sign position as a numeric digit and lose the reviewed truncation boundary',
        'if floating_symbol.is_some() {\n        let numeric_capacity = digit_positions',
        'if false {\n        let numeric_capacity = digit_positions',
    ),
    Mutation(
        'cobol-drop-floating-minus',
        'Suppress the negative sign produced by the product numeric-edited MOVE path',
        "            _ if value.coefficient < 0 => Some(b'-'),",
        "            _ if value.coefficient < 0 => Some(b' '),",
    ),
)

TYPED_ARITHMETIC_COMMAND = [
    'cargo', 'test', '--locked', '--offline', '-p', 'mainframe-env-interpreter',
    'machine::typed_decimal::tests::', '--', '--test-threads=1',
]
TYPED_ARITHMETIC_EXPECTED_TESTS = {
    'machine::typed_decimal::tests::typed_expression_uses_every_primitive_and_length_then_writes_exact_bytes',
    'machine::typed_decimal::tests::current_empty_assignment_plan_is_an_explicit_noop',
    'machine::typed_decimal::tests::all_rounding_policies_have_exact_directed_results',
    'machine::typed_decimal::tests::division_by_zero_is_a_size_error_and_does_not_mutate',
    'machine::typed_decimal::tests::typed_size_error_branching_ignores_changed_or_missing_control_text',
    'machine::typed_decimal::tests::missing_or_drifted_typed_size_error_metadata_is_rejected',
    'machine::typed_decimal::tests::receiver_local_size_error_preserves_only_the_failing_receiver',
    'machine::typed_decimal::tests::first_middle_and_last_receiver_failures_do_not_suppress_other_results',
    'machine::typed_decimal::tests::all_success_and_all_failure_receiver_batches_have_exact_results',
    'machine::typed_decimal::tests::receiver_overflow_without_size_error_handler_truncates_and_continues',
    'machine::typed_decimal::tests::operands_are_captured_before_any_receiver_is_written',
    'machine::typed_decimal::tests::shared_expression_failure_does_not_commit_any_receiver',
    'machine::typed_decimal::tests::malformed_plan_and_wrong_dialect_identity_fail_during_construction',
    'machine::typed_decimal::tests::storage_slot_and_qualified_name_must_identify_the_same_declared_view',
    'machine::typed_decimal::tests::equal_alias_views_cannot_substitute_a_different_storage_identity',
    'machine::typed_decimal::tests::legacy_arguments_are_rejected_even_when_the_typed_plan_is_valid',
    'machine::typed_decimal::tests::current_origin_is_provenance_and_does_not_select_execution',
    'machine::typed_decimal::tests::operation_plan_version_and_unknown_policy_fail_closed',
    'machine::typed_decimal::tests::explicit_arithmetic_context_changes_the_exact_result',
    'machine::typed_decimal::tests::historical_assign_v1_preserves_whole_batch_receiver_atomicity',
}
TYPED_ARITHMETIC_MUTATIONS = (
    Mutation(
        'typed-decimal-add-as-subtract',
        'Execute the shared typed decimal addition primitive as subtraction',
        'decimal_primitive_binary(mode, left, right, CobolArithmetic::add)',
        'decimal_primitive_binary(mode, left, right, CobolArithmetic::subtract)',
    ),
    Mutation(
        'typed-decimal-ignore-rounded',
        'Make AWAY-FROM-ZERO truncate instead of applying its reviewed rounding direction',
        'DecimalRoundingPolicy::AwayFromZero if remainder != 0 => sign,',
        'DecimalRoundingPolicy::AwayFromZero if remainder != 0 => 0,',
    ),
    Mutation(
        'typed-decimal-suppress-successful-receivers',
        'Restore whole-batch suppression when one receiver conversion raises SIZE ERROR',
        '    for (view, bytes) in staged {\n'
        '        machine.bases[view.base][view.offset..view.offset + view.length]'
        '.copy_from_slice(&bytes);\n'
        '    }\n'
        '    if receiver_size_error {',
        '    if !receiver_size_error {\n'
        '        for (view, bytes) in staged {\n'
        '            machine.bases[view.base][view.offset..view.offset + view.length]'
        '.copy_from_slice(&bytes);\n'
        '        }\n'
        '    }\n'
        '    if receiver_size_error {',
    ),
    Mutation(
        'typed-decimal-overwrite-failing-receiver',
        'Overwrite a receiver that failed conversion even when ON SIZE ERROR requires preservation',
        '                if !preserve_failed_receiver {\n'
        '                    staged.push(stage_receiver_truncated(',
        '                if true {\n'
        '                    staged.push(stage_receiver_truncated(',
    ),
    Mutation(
        'typed-decimal-preserve-no-handler-overflow',
        'Preserve a receiver overflow even though a missing ON SIZE ERROR phrase requires truncation and storage',
        '                if !preserve_failed_receiver {\n'
        '                    staged.push(stage_receiver_truncated(',
        '                if false {\n'
        '                    staged.push(stage_receiver_truncated(',
    ),
    Mutation(
        'typed-decimal-drop-size-error-condition',
        'Report success after receiver conversion failure, selecting the wrong condition branch',
        '    if receiver_size_error {\n'
        '        Ok(DecimalExecutionStatus::ReceiverSizeError)\n'
        '    } else {\n'
        '        Ok(DecimalExecutionStatus::Success)\n'
        '    }',
        '    let _ = receiver_size_error;\n'
        '    Ok(DecimalExecutionStatus::Success)',
    ),
    Mutation(
        'typed-decimal-write-before-operand-capture',
        'Evaluate and write receivers sequentially instead of capturing every operand first',
        '    let evaluated = plan\n'
        '        .assignments\n'
        '        .iter()\n'
        '        .map(|assignment| evaluate(machine, operation, &plan, &assignment.expression))\n'
        '        .collect::<Result<Vec<_>, _>>()?;\n'
        '    let mut staged = Vec::with_capacity(plan.assignments.len());\n'
        '    let mut receiver_size_error = false;\n'
        '    for (assignment, value) in plan.assignments.iter().zip(evaluated) {\n'
        '        match stage_receiver(\n'
        '            machine,\n'
        '            operation,\n'
        '            &assignment.receiver,\n'
        '            value,\n'
        '            preserve_failed_receiver,\n'
        '        ) {\n'
        '            Ok(write) => staged.push(write),',
        '    let mut staged = Vec::with_capacity(plan.assignments.len());\n'
        '    let mut receiver_size_error = false;\n'
        '    for assignment in &plan.assignments {\n'
        '        let value = evaluate(machine, operation, &plan, &assignment.expression)?;\n'
        '        match stage_receiver(\n'
        '            machine,\n'
        '            operation,\n'
        '            &assignment.receiver,\n'
        '            value,\n'
        '            preserve_failed_receiver,\n'
        '        ) {\n'
        '            Ok(write) => {\n'
        '                let (view, bytes) = &write;\n'
        '                machine.bases[view.base][view.offset..view.offset + view.length]\n'
        '                    .copy_from_slice(bytes);\n'
        '                staged.push(write);\n'
        '            }',
    ),
)

COBOL_CORRESPONDING_COMMAND = [
    'cargo', 'test', '--locked', '--offline', '-p', 'mainframe-env-conformance',
    'framework::tests::add_corresponding_', '--', '--test-threads=1',
]
COBOL_CORRESPONDING_EXPECTED_TESTS = {
    'framework::tests::add_corresponding_updates_matching_numeric_descendants',
    'framework::tests::add_corresponding_uses_relative_qualifiers_through_the_typed_route',
    'framework::tests::add_corresponding_compatibility_route_preserves_the_selected_occurrence',
    'framework::tests::add_corresponding_compatibility_route_honors_qualification_and_exclusions',
    'framework::tests::add_corresponding_requires_bilateral_uniqueness_at_execution',
    'framework::tests::add_corresponding_excludes_occurs_and_redefines_items_at_execution',
}
COBOL_CORRESPONDING_MUTATIONS = (
    Mutation(
        'cobol-corresponding-leaf-only-match',
        'Drop relative qualifiers and match ADD CORRESPONDING items by leaf name alone',
        '            key.push(qualifier.name.clone());',
        '            let _ = qualifier;',
    ),
    Mutation(
        'cobol-corresponding-omit-occurs-exclusion',
        'Treat an OCCURS item or a descendant of an OCCURS group as corresponding',
        '            || subordinate.occurs_clause',
        '            || false',
    ),
    Mutation(
        'cobol-corresponding-omit-uniqueness',
        'Pair the first source even when either relative qualified name is not unique',
        '(matches.len() == 1 && target_counts.get(&key) == Some(&1)).then(|| {',
        '(!matches.is_empty()).then(|| {',
    ),
)


def digest(data: bytes) -> str:
    return 'sha256:' + hashlib.sha256(data).hexdigest()


def split_test_code(source: str) -> tuple[str, str]:
    """Skip test-only imports and external module declarations as boundaries."""
    marker = '#[cfg(test)]'
    for match in re.finditer(re.escape(marker), source):
        tail = source[match.end():].lstrip()
        if re.match(r'(?:pub(?:\([^)]*\))?\s+)?use\b', tail):
            continue
        if re.match(r'(?:pub(?:\([^)]*\))?\s+)?mod\s+\w+\s*;', tail):
            continue
        return source[:match.start()], source[match.start():]
    return source, ''


def apply_mutation(source: str, mutation: Mutation) -> str:
    implementation, tests = split_test_code(source)
    if implementation.count(mutation.old) != 1 or mutation.old == mutation.new:
        raise ValueError('mutation anchor must match exactly once in implementation, not tests')
    anchor_start = implementation.index(mutation.old)
    anchor_end = anchor_start + len(mutation.old)
    for imported in re.finditer(
        r'#\[cfg\(test\)\]\s*(?:pub(?:\([^)]*\))?\s+)?(?:use\b[^;]*|mod\s+\w+\s*);',
        implementation,
    ):
        if anchor_start < imported.end() and anchor_end > imported.start():
            raise ValueError('mutation anchor overlaps a test-only declaration')
    changed = implementation.replace(mutation.old, mutation.new, 1) + tests
    if split_test_code(changed)[1] != tests:
        raise ValueError('mutation changed ordinary test code')
    return changed


def classify(returncode: int | None, output: str, expected: set[str]) -> tuple[str, list[str]]:
    """Only a runtime assertion failure from the unchanged test set kills a mutant."""
    if returncode is None:
        return 'timed_out', []
    clean = ANSI.sub('', output)
    outcomes = dict(TEST.findall(clean))
    if set(outcomes) != expected or 'could not compile' in clean or re.search(r'error\[E\d+\]', clean):
        return 'invalid', []
    failed = sorted(name for name, outcome in outcomes.items() if outcome == 'FAILED')
    if returncode == 0 and not failed and 'test result: ok.' in clean:
        return 'survived', []
    if returncode != 0 and failed and 'test result: FAILED.' in clean:
        # Test harness assertion panics are required; an abort/signal isn't kill evidence.
        if all(f"thread '{name}'" in clean and 'panicked at' in clean for name in failed):
            return 'killed', failed
    return 'invalid', []


def execute(cwd: Path, log: Path, timeout: int, command: list[str] = COMMAND,
            expected: set[str] = EXPECTED_TESTS) -> dict:
    started = time.monotonic()
    environment = os.environ.copy()
    environment['CARGO_TERM_COLOR'] = 'never'
    environment['RUST_BACKTRACE'] = '0'
    environment['CARGO_INCREMENTAL'] = '0'
    # Both successful and failed mutants use the exact same unmodified test command.
    try:
        with log.open('wb') as stream:
            completed = subprocess.run(command, cwd=cwd, env=environment, stdout=stream,
                                       stderr=subprocess.STDOUT, timeout=timeout, check=False)
        code = completed.returncode
    except subprocess.TimeoutExpired:
        code = None
    content = log.read_bytes()
    classification, killed_by = classify(code, content.decode(errors='replace'), expected)
    return {'exit_code': code, 'duration_seconds': round(time.monotonic() - started, 3),
            'classification': classification, 'killing_tests': killed_by,
            'log': log.name, 'log_digest': digest(content)}


def campaign(root: Path, output: Path, timeout: int) -> int:
    if subprocess.check_output(['git', 'status', '--porcelain'], cwd=root).strip():
        raise ValueError('candidate must be clean: commit the intended changes before the campaign')
    candidate = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=root, text=True).strip()
    tree = subprocess.check_output(['git', 'rev-parse', 'HEAD^{tree}'], cwd=root, text=True).strip()
    archive = subprocess.check_output(['git', 'archive', '--format=tar', candidate], cwd=root)
    if len(archive) > 128 * 1024 * 1024:
        raise ValueError('source archive exceeds the bounded campaign limit')
    original = subprocess.check_output(['git', 'show', f'{candidate}:{SOURCE.as_posix()}'], cwd=root).decode()
    cics_original = {
        source: subprocess.check_output(
            ['git', 'show', f'{candidate}:{source.as_posix()}'], cwd=root
        ).decode()
        for source in CICS_SOURCES
    }
    cics_scenarios = subprocess.check_output(
        ['git', 'show', f'{candidate}:{CICS_SCENARIOS.as_posix()}'], cwd=root
    )
    cobol_move_original = subprocess.check_output(
        ['git', 'show', f'{candidate}:{COBOL_MOVE_SOURCE.as_posix()}'], cwd=root
    ).decode()
    cobol_move_scenarios = subprocess.check_output(
        ['git', 'show', f'{candidate}:{COBOL_MOVE_SCENARIOS.as_posix()}'], cwd=root
    )
    typed_arithmetic_original = subprocess.check_output(
        ['git', 'show', f'{candidate}:{TYPED_ARITHMETIC_SOURCE.as_posix()}'], cwd=root
    ).decode()
    typed_arithmetic_tests = typed_arithmetic_original.partition('#[cfg(test)]')[2].encode()
    cobol_corresponding_original = subprocess.check_output(
        ['git', 'show', f'{candidate}:{COBOL_CORRESPONDING_SOURCE.as_posix()}'], cwd=root
    ).decode()
    cobol_corresponding_scenarios = subprocess.check_output(
        ['git', 'show', f'{candidate}:{COBOL_CORRESPONDING_SCENARIOS.as_posix()}'], cwd=root
    )
    output.mkdir(parents=True, exist_ok=False)
    receipt = {'schema_version': SCHEMA_VERSION,
               'candidate': candidate, 'tree': tree, 'model_kind': 'independent-reference-source',
               'scope': 'model transitions, not product-runtime mutants',
               'product_mutation_credit': 0, 'licensed_differential_credit': 0,
               'source': SOURCE.as_posix(), 'original_source_digest': digest(original.encode()),
               'unchanged_tests_digest': digest(original.partition('#[cfg(test)]')[2].encode()),
               'command': COMMAND, 'timeout_seconds': timeout,
               'rustc': subprocess.check_output(['rustc', '-Vv'], text=True),
               'cargo': subprocess.check_output(['cargo', '-V'], text=True).strip(),
               'mutants': [], 'equivalence_assessment': 'not inferred; surviving mutants require review',
               'product_runtime': {
                   'model_kind': 'production-runtime-source',
                   'scope': 'CICS file/UOW product code traversed by the unchanged normal pilot',
                   'product_mutation_credit': 1,
                   'licensed_differential_credit': 0,
                   'sources': [source.as_posix() for source in CICS_SOURCES],
                   'original_source_digests': {
                       source.as_posix(): digest(value.encode())
                       for source, value in cics_original.items()
                   },
                   'unchanged_scenarios': CICS_SCENARIOS.as_posix(),
                   'unchanged_scenarios_digest': digest(cics_scenarios),
                   'command': CICS_COMMAND,
                   'timeout_seconds': timeout,
                   'mutants': [],
                   'equivalence_assessment': 'not inferred; surviving mutants require review',
               },
               'cobol_runtime': {
                   'model_kind': 'production-runtime-source',
                   'scope': 'COBOL numeric-edited MOVE product code traversed by the unchanged second-family pilot',
                   'product_mutation_credit': 1,
                   'licensed_differential_credit': 0,
                   'source': COBOL_MOVE_SOURCE.as_posix(),
                   'original_source_digest': digest(cobol_move_original.encode()),
                   'unchanged_scenarios': COBOL_MOVE_SCENARIOS.as_posix(),
                   'unchanged_scenarios_digest': digest(cobol_move_scenarios),
                   'command': COBOL_MOVE_COMMAND,
                   'timeout_seconds': timeout,
                   'mutants': [],
                   'equivalence_assessment': 'not inferred; surviving mutants require review',
               },
               'typed_arithmetic_runtime': {
                   'model_kind': 'production-runtime-source',
                   'scope': 'typed decimal arithmetic, receiver-local conversion, condition, and operand-capture semantics',
                   'product_mutation_credit': 1,
                   'licensed_differential_credit': 0,
                   'source': TYPED_ARITHMETIC_SOURCE.as_posix(),
                   'original_source_digest': digest(typed_arithmetic_original.encode()),
                   'unchanged_tests_digest': digest(typed_arithmetic_tests),
                   'command': TYPED_ARITHMETIC_COMMAND,
                   'timeout_seconds': timeout,
                   'mutants': [],
                   'equivalence_assessment': 'not inferred; surviving mutants require review',
               },
               'cobol_corresponding_compiler': {
                   'model_kind': 'production-compiler-source',
                   'scope': 'ADD CORRESPONDING relative qualifiers, eligibility, and bilateral uniqueness',
                   'product_mutation_credit': 1,
                   'licensed_differential_credit': 0,
                   'source': COBOL_CORRESPONDING_SOURCE.as_posix(),
                   'original_source_digest': digest(cobol_corresponding_original.encode()),
                   'unchanged_scenarios': COBOL_CORRESPONDING_SCENARIOS.as_posix(),
                   'unchanged_scenarios_digest': digest(cobol_corresponding_scenarios),
                   'command': COBOL_CORRESPONDING_COMMAND,
                   'timeout_seconds': timeout,
                   'mutants': [],
                   'equivalence_assessment': 'not inferred; surviving mutants require review',
               }}

    def save() -> None:
        (output / 'receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')

    with tempfile.TemporaryDirectory(prefix='mainframe-dataset-mutations-') as directory:
        snapshot = Path(directory)
        with tarfile.open(fileobj=io.BytesIO(archive), mode='r:') as tar:
            # The extraction filter arrived in 3.11.4; the bookworm interpreter
            # that runs this gate in CI is 3.11.2. tarfile.data_filter is the
            # documented way to detect support. The archive is produced by
            # git archive over the candidate a few lines above, not untrusted
            # input, so extracting unfiltered on older interpreters is sound.
            if hasattr(tarfile, 'data_filter'):
                tar.extractall(snapshot, filter='data')
            else:
                tar.extractall(snapshot)
        receipt['baseline'] = execute(snapshot, output / 'baseline.log', timeout)
        save()
        if receipt['baseline']['classification'] != 'survived':
            raise ValueError('unchanged baseline tests did not pass; no mutant receives credit')
        for mutation in MUTATIONS:
            entry = {'id': mutation.identity, 'behavior': mutation.behavior,
                     'old': mutation.old, 'new': mutation.new}
            receipt['mutants'].append(entry)
            try:
                changed = apply_mutation(original, mutation)
                (snapshot / SOURCE).write_text(changed)
                entry['mutated_source_digest'] = digest(changed.encode())
                entry.update(execute(snapshot, output / (mutation.identity + '.log'), timeout))
            except (ValueError, OSError) as error:
                entry.update(classification='invalid', error=str(error), killing_tests=[])
            finally:
                (snapshot / SOURCE).write_text(original)
                save()
            print(f"{mutation.identity}: {entry['classification']}", flush=True)
        product = receipt['product_runtime']
        product['baseline'] = execute(
            snapshot, output / 'cics-baseline.log', timeout, CICS_COMMAND, CICS_EXPECTED_TESTS
        )
        save()
        if product['baseline']['classification'] != 'survived':
            raise ValueError('unchanged CICS pilot did not pass; no product mutant receives credit')
        for mutation in CICS_MUTATIONS:
            source = mutation.source
            if source is None:
                raise ValueError(f'CICS mutation {mutation.identity} has no source module')
            original_source = cics_original[source]
            entry = {'id': mutation.identity, 'behavior': mutation.behavior,
                     'source': source.as_posix(), 'old': mutation.old, 'new': mutation.new}
            product['mutants'].append(entry)
            try:
                changed = apply_mutation(original_source, mutation)
                (snapshot / source).write_text(changed)
                entry['mutated_source_digest'] = digest(changed.encode())
                entry.update(execute(
                    snapshot, output / (mutation.identity + '.log'), timeout,
                    CICS_COMMAND, CICS_EXPECTED_TESTS,
                ))
            except (ValueError, OSError) as error:
                entry.update(classification='invalid', error=str(error), killing_tests=[])
            finally:
                (snapshot / source).write_text(original_source)
                save()
            print(f"{mutation.identity}: {entry['classification']}", flush=True)
        cobol = receipt['cobol_runtime']
        cobol['baseline'] = execute(
            snapshot, output / 'cobol-move-baseline.log', timeout,
            COBOL_MOVE_COMMAND, COBOL_MOVE_EXPECTED_TESTS,
        )
        save()
        if cobol['baseline']['classification'] != 'survived':
            raise ValueError('unchanged COBOL MOVE pilot did not pass; no product mutant receives credit')
        for mutation in COBOL_MOVE_MUTATIONS:
            entry = {'id': mutation.identity, 'behavior': mutation.behavior,
                     'old': mutation.old, 'new': mutation.new}
            cobol['mutants'].append(entry)
            try:
                changed = apply_mutation(cobol_move_original, mutation)
                (snapshot / COBOL_MOVE_SOURCE).write_text(changed)
                entry['mutated_source_digest'] = digest(changed.encode())
                entry.update(execute(
                    snapshot, output / (mutation.identity + '.log'), timeout,
                    COBOL_MOVE_COMMAND, COBOL_MOVE_EXPECTED_TESTS,
                ))
            except (ValueError, OSError) as error:
                entry.update(classification='invalid', error=str(error), killing_tests=[])
            finally:
                (snapshot / COBOL_MOVE_SOURCE).write_text(cobol_move_original)
                save()
            print(f"{mutation.identity}: {entry['classification']}", flush=True)
        arithmetic = receipt['typed_arithmetic_runtime']
        arithmetic['baseline'] = execute(
            snapshot, output / 'typed-arithmetic-baseline.log', timeout,
            TYPED_ARITHMETIC_COMMAND, TYPED_ARITHMETIC_EXPECTED_TESTS,
        )
        save()
        if arithmetic['baseline']['classification'] != 'survived':
            raise ValueError(
                'unchanged typed arithmetic test did not pass; no product mutant receives credit'
            )
        for mutation in TYPED_ARITHMETIC_MUTATIONS:
            entry = {'id': mutation.identity, 'behavior': mutation.behavior,
                     'old': mutation.old, 'new': mutation.new}
            arithmetic['mutants'].append(entry)
            try:
                changed = apply_mutation(typed_arithmetic_original, mutation)
                (snapshot / TYPED_ARITHMETIC_SOURCE).write_text(changed)
                entry['mutated_source_digest'] = digest(changed.encode())
                entry.update(execute(
                    snapshot, output / (mutation.identity + '.log'), timeout,
                    TYPED_ARITHMETIC_COMMAND, TYPED_ARITHMETIC_EXPECTED_TESTS,
                ))
            except (ValueError, OSError) as error:
                entry.update(classification='invalid', error=str(error), killing_tests=[])
            finally:
                (snapshot / TYPED_ARITHMETIC_SOURCE).write_text(typed_arithmetic_original)
                save()
            print(f"{mutation.identity}: {entry['classification']}", flush=True)
        corresponding = receipt['cobol_corresponding_compiler']
        corresponding['baseline'] = execute(
            snapshot, output / 'cobol-corresponding-baseline.log', timeout,
            COBOL_CORRESPONDING_COMMAND, COBOL_CORRESPONDING_EXPECTED_TESTS,
        )
        save()
        if corresponding['baseline']['classification'] != 'survived':
            raise ValueError(
                'unchanged ADD CORRESPONDING tests did not pass; no compiler mutant receives credit'
            )
        for mutation in COBOL_CORRESPONDING_MUTATIONS:
            entry = {'id': mutation.identity, 'behavior': mutation.behavior,
                     'old': mutation.old, 'new': mutation.new}
            corresponding['mutants'].append(entry)
            try:
                changed = apply_mutation(cobol_corresponding_original, mutation)
                (snapshot / COBOL_CORRESPONDING_SOURCE).write_text(changed)
                entry['mutated_source_digest'] = digest(changed.encode())
                entry.update(execute(
                    snapshot, output / (mutation.identity + '.log'), timeout,
                    COBOL_CORRESPONDING_COMMAND, COBOL_CORRESPONDING_EXPECTED_TESTS,
                ))
            except (ValueError, OSError) as error:
                entry.update(classification='invalid', error=str(error), killing_tests=[])
            finally:
                (snapshot / COBOL_CORRESPONDING_SOURCE).write_text(
                    cobol_corresponding_original
                )
                save()
            print(f"{mutation.identity}: {entry['classification']}", flush=True)
    receipt['counts'] = {state: sum(m['classification'] == state for m in receipt['mutants'])
                         for state in ('killed', 'survived', 'invalid', 'timed_out')}
    receipt['product_runtime']['counts'] = {
        state: sum(m['classification'] == state for m in receipt['product_runtime']['mutants'])
        for state in ('killed', 'survived', 'invalid', 'timed_out')
    }
    receipt['cobol_runtime']['counts'] = {
        state: sum(m['classification'] == state for m in receipt['cobol_runtime']['mutants'])
        for state in ('killed', 'survived', 'invalid', 'timed_out')
    }
    receipt['typed_arithmetic_runtime']['counts'] = {
        state: sum(
            m['classification'] == state
            for m in receipt['typed_arithmetic_runtime']['mutants']
        )
        for state in ('killed', 'survived', 'invalid', 'timed_out')
    }
    receipt['cobol_corresponding_compiler']['counts'] = {
        state: sum(
            m['classification'] == state
            for m in receipt['cobol_corresponding_compiler']['mutants']
        )
        for state in ('killed', 'survived', 'invalid', 'timed_out')
    }
    receipt['success'] = (
        receipt['counts']['killed'] == len(MUTATIONS)
        and receipt['product_runtime']['counts']['killed'] == len(CICS_MUTATIONS)
        and receipt['cobol_runtime']['counts']['killed'] == len(COBOL_MOVE_MUTATIONS)
        and receipt['typed_arithmetic_runtime']['counts']['killed']
        == len(TYPED_ARITHMETIC_MUTATIONS)
        and receipt['cobol_corresponding_compiler']['counts']['killed']
        == len(COBOL_CORRESPONDING_MUTATIONS)
    )
    save()
    print(json.dumps(receipt['counts'], sort_keys=True))
    print(json.dumps(receipt['product_runtime']['counts'], sort_keys=True))
    print(json.dumps(receipt['cobol_runtime']['counts'], sort_keys=True))
    print(json.dumps(receipt['typed_arithmetic_runtime']['counts'], sort_keys=True))
    print(json.dumps(receipt['cobol_corresponding_compiler']['counts'], sort_keys=True))
    return 0 if receipt['success'] else 1


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True,
                        help='new receipt/log directory outside tracked source (e.g. target/mutations/run-1)')
    parser.add_argument('--timeout', type=int, default=300, help='seconds per compile/test (1..1800)')
    args = parser.parse_args()
    if not 1 <= args.timeout <= 1800:
        parser.error('timeout must be between 1 and 1800 seconds')
    root = Path(__file__).resolve().parents[1]
    try:
        return campaign(root, args.output.resolve(), args.timeout)
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        parser.exit(2, f'mutation campaign stopped: {error}\n')


if __name__ == '__main__':
    raise SystemExit(main())
