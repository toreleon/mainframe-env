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
TEST_PREFIX = 'dataset_reference::tests::'
COMMAND = ['cargo', 'test', '--locked', '-p', 'mainframe-env-conformance',
           TEST_PREFIX, '--', '--test-threads=1']
ANSI = re.compile(r'\x1b\[[0-9;]*m')
TEST = re.compile(r'^test (dataset_reference::tests::\S+) \.\.\. (ok|FAILED)$', re.M)
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


def execute(cwd: Path, log: Path, timeout: int) -> dict:
    started = time.monotonic()
    environment = os.environ.copy()
    environment['CARGO_TERM_COLOR'] = 'never'
    environment['RUST_BACKTRACE'] = '0'
    environment['CARGO_INCREMENTAL'] = '0'
    # Both successful and failed mutants use the exact same unmodified test command.
    try:
        with log.open('wb') as stream:
            completed = subprocess.run(COMMAND, cwd=cwd, env=environment, stdout=stream,
                                       stderr=subprocess.STDOUT, timeout=timeout, check=False)
        code = completed.returncode
    except subprocess.TimeoutExpired:
        code = None
    content = log.read_bytes()
    classification, killed_by = classify(code, content.decode(errors='replace'), EXPECTED_TESTS)
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
    output.mkdir(parents=True, exist_ok=False)
    receipt = {'schema_version': 'mainframe-env.dataset-source-mutations@1',
               'candidate': candidate, 'tree': tree, 'model_kind': 'independent-reference-source',
               'scope': 'model transitions, not product-runtime mutants',
               'product_mutation_credit': 0, 'licensed_differential_credit': 0,
               'source': SOURCE.as_posix(), 'original_source_digest': digest(original.encode()),
               'unchanged_tests_digest': digest(original.partition('#[cfg(test)]')[2].encode()),
               'command': COMMAND, 'timeout_seconds': timeout,
               'rustc': subprocess.check_output(['rustc', '-Vv'], text=True),
               'cargo': subprocess.check_output(['cargo', '-V'], text=True).strip(),
               'mutants': [], 'equivalence_assessment': 'not inferred; surviving mutants require review'}

    def save() -> None:
        (output / 'receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')

    with tempfile.TemporaryDirectory(prefix='mainframe-dataset-mutations-') as directory:
        snapshot = Path(directory)
        with tarfile.open(fileobj=io.BytesIO(archive), mode='r:') as tar:
            tar.extractall(snapshot, filter='data')
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
    receipt['counts'] = {state: sum(m['classification'] == state for m in receipt['mutants'])
                         for state in ('killed', 'survived', 'invalid', 'timed_out')}
    receipt['success'] = receipt['counts']['killed'] == len(MUTATIONS)
    save()
    print(json.dumps(receipt['counts'], sort_keys=True))
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
