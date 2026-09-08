#!/usr/bin/env python3
"""Candidate-bound behavioral mutation tests of the independent dataset model.

Mutate transition *source*, never observations, test assertions or live product
state. A compiler failure, missing assertion, or timeout receives no kill credit.
Receipts are local-model evidence; they grant zero product/IBM differential credit.
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
CICS_SOURCE = Path('crates/providers/mainframe-env-cics/src/service.rs')
CICS_SCENARIOS = Path('crates/tooling/mainframe-env-conformance/src/cics_pilot.rs')
COBOL_MOVE_SOURCE = Path('crates/kernel/mainframe-env-interpreter/src/machine.rs')
COBOL_MOVE_SCENARIOS = Path('crates/tooling/mainframe-env-conformance/src/cobol_move_pilot.rs')
TEST_PREFIX = 'dataset_reference::tests::'
COMMAND = ['cargo', 'test', '--locked', '-p', 'mainframe-env-conformance',
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
    'cargo', 'test', '--locked', '-p', 'mainframe-env-conformance',
    'cics_pilot::tests::cics_pilot_runs_compiler_interpreter_providers_and_real_stores',
    '--', '--exact', '--test-threads=1',
]
CICS_EXPECTED_TESTS = {
    'cics_pilot::tests::cics_pilot_runs_compiler_interpreter_providers_and_real_stores'
}
CICS_MUTATIONS = (
    Mutation(
        'cics-omit-rewrite',
        'Report a successful product REWRITE without invoking the dataset transition',
        'let result = self\n            .nested(run, HostRequest::Dataset(host_request))',
        'let result = if operation == CicsOperation::Rewrite {\n'
        '            Ok(HostResult::Dataset(DatasetResult::Mutated { version: 1 }))\n'
        '        } else {\n'
        '            self.nested(run, HostRequest::Dataset(host_request))\n'
        '        }',
    ),
    Mutation(
        'cics-rollback-noop',
        'Turn product rollback into a durable-undo discard',
        'if outcome == CicsUnitOfWorkOutcome::RolledBack {\n            self.rollback_run(run)?;',
        'if outcome == CicsUnitOfWorkOutcome::RolledBack {\n            self.clear_undo(run)?;',
    ),
    Mutation(
        'cics-bypass-read-update',
        'Allow a plain READ identity to establish REWRITE context',
        'if request.arguments.contains_key("OPTION.UPDATE")\n                    && let Some(identity) = identities.first()',
        'if let Some(identity) = identities.first()',
    ),
)

COBOL_MOVE_COMMAND = [
    'cargo', 'test', '--locked', '-p', 'mainframe-env-conformance',
    'cobol_move_pilot::tests::cobol_numeric_move_runs_source_to_exact_independent_bytes',
    '--', '--exact', '--test-threads=1',
]
COBOL_MOVE_EXPECTED_TESTS = {
    'cobol_move_pilot::tests::cobol_numeric_move_runs_source_to_exact_independent_bytes'
}
COBOL_MOVE_MUTATIONS = (
    Mutation(
        'cobol-drop-floating-minus',
        'Suppress the negative sign produced by the product numeric-edited MOVE path',
        "if floating_sign_slot == Some(digit_index) {\n"
        "                    output.push(if value.coefficient < 0 { b'-' } else { b'+' });",
        "if floating_sign_slot == Some(digit_index) {\n"
        "                    output.push(if value.coefficient < 0 { b' ' } else { b'+' });",
    ),
)


def digest(data: bytes) -> str:
    return 'sha256:' + hashlib.sha256(data).hexdigest()


def apply_mutation(source: str, mutation: Mutation) -> str:
    implementation, marker, tests = source.partition('#[cfg(test)]')
    if not marker or implementation.count(mutation.old) != 1 or mutation.old == mutation.new:
        raise ValueError('mutation anchor must match exactly once in implementation, not tests')
    changed = implementation.replace(mutation.old, mutation.new, 1) + marker + tests
    if changed.partition(marker)[2] != tests:
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
    cics_original = subprocess.check_output(
        ['git', 'show', f'{candidate}:{CICS_SOURCE.as_posix()}'], cwd=root
    ).decode()
    cics_scenarios = subprocess.check_output(
        ['git', 'show', f'{candidate}:{CICS_SCENARIOS.as_posix()}'], cwd=root
    )
    cobol_move_original = subprocess.check_output(
        ['git', 'show', f'{candidate}:{COBOL_MOVE_SOURCE.as_posix()}'], cwd=root
    ).decode()
    cobol_move_scenarios = subprocess.check_output(
        ['git', 'show', f'{candidate}:{COBOL_MOVE_SCENARIOS.as_posix()}'], cwd=root
    )
    output.mkdir(parents=True, exist_ok=False)
    receipt = {'schema_version': 'mainframe-env.source-mutations@2',
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
                   'source': CICS_SOURCE.as_posix(),
                   'original_source_digest': digest(cics_original.encode()),
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
            entry = {'id': mutation.identity, 'behavior': mutation.behavior,
                     'old': mutation.old, 'new': mutation.new}
            product['mutants'].append(entry)
            try:
                changed = apply_mutation(cics_original, mutation)
                (snapshot / CICS_SOURCE).write_text(changed)
                entry['mutated_source_digest'] = digest(changed.encode())
                entry.update(execute(
                    snapshot, output / (mutation.identity + '.log'), timeout,
                    CICS_COMMAND, CICS_EXPECTED_TESTS,
                ))
            except (ValueError, OSError) as error:
                entry.update(classification='invalid', error=str(error), killing_tests=[])
            finally:
                (snapshot / CICS_SOURCE).write_text(cics_original)
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
    receipt['success'] = (
        receipt['counts']['killed'] == len(MUTATIONS)
        and receipt['product_runtime']['counts']['killed'] == len(CICS_MUTATIONS)
        and receipt['cobol_runtime']['counts']['killed'] == len(COBOL_MOVE_MUTATIONS)
    )
    save()
    print(json.dumps(receipt['counts'], sort_keys=True))
    print(json.dumps(receipt['product_runtime']['counts'], sort_keys=True))
    print(json.dumps(receipt['cobol_runtime']['counts'], sort_keys=True))
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
