#!/usr/bin/env python3
"""Bounded local CI selection; full/release assurance remains in Jenkinsfile."""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools'))
import ci_assurance as ci

POLICY = ['supply-chain', 'cargo-deny', 'license-notices', 'docs', 'python-tooling-tests']
RUNTIME = ['fmt', 'msrv', 'tests', 'clippy', 'postgres-parity']
NORMATIVE = ('docs/contracts/', 'docs/architecture/', 'docs/decisions/',
             'docs/compatibility/', 'docs/releases/')
COMMANDS = {
    'supply-chain': ['python3', '-B', 'tools/supply_chain.py', 'check'],
    'cargo-deny': ['cargo', 'deny', 'check'],
    'license-notices': ['cargo', 'xtask', 'license-notices', '--check'],
    'docs': ['cargo', 'xtask', 'docs', '--check'],
    'python-tooling-tests': ['python3', '-B', 'tools/run_tooling_tests.py'],
    'fmt': ['cargo', 'fmt', '--all', '--', '--check'],
    'msrv': ['cargo', '+1.95.0', 'check', '--workspace', '--all-targets', '--all-features', '--locked'],
    'tests': ['bash', '/opt/mainframe-env/docker/test-workspace.sh'],
    'clippy': ['cargo', 'clippy', '--workspace', '--all-targets', '--all-features', '--locked', '--', '-D', 'warnings'],
    'postgres-parity': ['tools/jenkins/postgres_parity.sh', 'run'],
}
TEST_GATES = {'python-tooling-tests', 'tests', 'postgres-parity'}


def plan(root: Path, mode: str, successful_base: str | None) -> dict:
    if ci.git(root, 'status', '--porcelain', '--untracked-files=normal'):
        raise ValueError('CI selection requires a clean committed checkout')
    if mode not in {'auto', 'runtime'}:
        raise ValueError('check mode must be auto or runtime')
    base = successful_base or None
    reason = 'missing-successful-base'
    if base:
        if not ci.SHA.fullmatch(base) or base == '0' * 40:
            raise ValueError('successful base must be a full lowercase commit SHA')
        result = subprocess.run(['git', 'merge-base', '--is-ancestor', base, 'HEAD'],
                                cwd=root, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        if result.returncode:
            base = None
            reason = 'unavailable-or-nonancestor-successful-base'
        else:
            reason = 'since-last-success'
    selected = ci.make_plan(root, {}, 'push', 'refs/heads/main', base, 'jenkins')
    build = selected['build']
    # Normative prose can change contracts or release obligations. Its own
    # documentation checks alone are insufficient to narrow runtime validation.
    registry_path = 'docs/documentation-registry.json'
    try:
        normative = json.loads((root / registry_path).read_text())['normative_documents']
        if not isinstance(normative, list) or not normative or not all(isinstance(p, str) for p in normative):
            raise ValueError('invalid normative document registry')
    except (OSError, ValueError, KeyError, TypeError):
        normative = []
        build = True
        reason = 'unavailable-normative-registry'
    if any(path.startswith(NORMATIVE) or path in normative or path == registry_path
           for path in selected['paths']):
        build = True
        reason = 'normative-change'
    if selected['selection_reason'] == 'unavailable-diff-select-all':
        reason = selected['selection_reason']
    if mode == 'runtime':
        build = True
        reason = 'explicit-runtime-checks'
    return {
        **ci.identity(root), 'schema_version': 'mainframe-env.local-ci-plan@1',
        'mode': mode, 'base': base, 'requested_successful_base': successful_base or None,
        'paths': selected['paths'],
        'selection_reason': reason, 'build': build, 'msrv': build, 'store': build,
        'docs': True, 'full': False, 'primary_gates': POLICY + (RUNTIME if build else []),
        'licensed_credit': 0, 'release_acceptance': False,
    }


def validate(root: Path, value: dict) -> None:
    if value.get('schema_version') != 'mainframe-env.local-ci-plan@1':
        raise ValueError('not a local CI plan')
    if any(value.get(key) != item for key, item in ci.identity(root).items()):
        raise ValueError('local CI plan belongs to a different candidate')
    if not isinstance(value.get('build'), bool):
        raise ValueError('build selector must be boolean')
    expected = plan(root, value['mode'], value['requested_successful_base'])
    if value != expected:
        raise ValueError('local CI plan does not match the current selection')


def check(root: Path, output: Path, value: dict) -> int:
    validate(root, value)
    gates = value['primary_gates']
    # Reusing logs/receipts from a previous run is forbidden, including retries
    # of the same candidate. Command receipts are evidence, not a test cache.
    for gate in COMMANDS:
        for suffix in ('.json', '.log'):
            (output / (gate + suffix)).unlink(missing_ok=True)
    try:
        for gate in gates:
            code = ci.record(root, output, gate, COMMANDS[gate], gate in TEST_GATES)
            if code:
                return code
        return 0
    finally:
        summary = ci.summarize(value, output, gates)
        summary['tier'] = 'local-runtime' if value['build'] else 'local-docs'
        summary['duration_seconds'] = round(sum(
            result.get('duration_seconds', 0) for result in summary['checks'].values()
        ), 3)
        ci.write_json(output / 'summary.json', summary)
        print(json.dumps({'tier': summary['tier'], 'seconds': summary['duration_seconds'],
                          'passed': summary['selected_commands_passed']}))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=['plan', 'check', 'build-required'])
    parser.add_argument('--output', type=Path, default=Path('/target/ci-assurance'))
    parser.add_argument('--mode', choices=['auto', 'runtime'],
                        default=os.environ.get('MAINFRAME_ENV_CI_CHECK_MODE', 'auto'))
    parser.add_argument('--successful-base', default=os.environ.get('GIT_PREVIOUS_SUCCESSFUL_COMMIT'))
    args = parser.parse_args()
    path = args.output / 'plan.json'
    if args.action == 'plan':
        value = plan(ROOT, args.mode, args.successful_base)
        ci.write_json(path, value)
        print(json.dumps(value, sort_keys=True))
        return 0
    value = json.loads(path.read_text())
    validate(ROOT, value)
    if args.action == 'build-required':
        print(str(value['build']).lower())
        return 0
    return check(ROOT, args.output, value)


if __name__ == '__main__':
    try:
        sys.exit(main())
    except (OSError, ValueError, KeyError, subprocess.CalledProcessError) as error:
        raise SystemExit(f'Local CI plan refused: {error}')
