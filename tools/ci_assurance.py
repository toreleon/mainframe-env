"""Cost-aware CI selection and exact-candidate command receipts (standard library only)."""
from __future__ import annotations
import argparse
import hashlib
import json
import os
import platform
import shutil
from pathlib import Path, PurePosixPath
import re
import subprocess
import sys
import time

ALL = frozenset({'architecture', 'evidence', 'runtime', 'compiler', 'store', 'mutation'})
SHARED = {
    'Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml', 'rustfmt.toml',
    'clippy.toml', 'deny.toml', 'release.toml', 'VERSION', 'Jenkinsfile',
}
PROSE = {'README.md', 'CHANGELOG.md', 'LICENSE', 'LICENSE.md', 'CONTRIBUTING.md', 'AGENTS.md'}
PRIMARY = ['fmt', 'spec', 'cobol', 'tests', 'clippy']
POLICY = ['cargo-deny', 'license-notices']
FULL = ['targets', 'documentation', 'conformance', 'certification', 'evidence-seal', 'runtime-architecture']
SHA = re.compile(r'[0-9a-f]{40}\Z')
EVENTS = frozenset({'local', 'push', 'pull_request', 'schedule', 'manual', 'tag'})


def obligations(paths: list[str]) -> list[str]:
    selected: set[str] = set()
    for path in paths:
        p = PurePosixPath(path)
        if not path or p.is_absolute() or '..' in p.parts or '\x00' in path or '\\' in path:
            raise ValueError('unsafe changed path')
        if path in SHARED or path.startswith(('.cargo/', '.github/', 'xtask/', 'tools/', 'crates/contracts/', 'crates/foundation/')):
            selected.update(ALL)
        elif path.startswith(('docs/contracts/', 'docs/architecture/', 'docs/decisions/', 'docs/compatibility/', 'docs/generated/', 'conformance/spec/')):
            selected.update(ALL)
        elif path.startswith(('conformance/', 'release/', 'docs/releases/')):
            # Evidence and obligations can refer to any subsystem. Prefer a bounded
            # superset to skipping a shared-contract obligation.
            selected.update(ALL)
        elif path.startswith('crates/stores/'):
            selected.update({'architecture', 'evidence', 'store', 'runtime'})
        elif path.startswith('crates/providers/'):
            selected.update({'architecture', 'evidence', 'store', 'runtime', 'mutation'})
        elif path.startswith('crates/kernel/mainframe-env-compiler'):
            selected.update({'architecture', 'evidence', 'compiler'})
        elif path.startswith(('crates/', 'config/')):
            selected.update({'architecture', 'evidence', 'runtime'})
        elif path in PROSE or (path.endswith('.md') and path.startswith(('docs/research/', 'docs/prompts/', 'docs/runbooks/', 'docs/delivery/'))):
            pass
        else:
            selected.update(ALL)  # New/unclassified paths never silently select no gate.
    return sorted(selected)


def git(root: Path, *args: str) -> str:
    return subprocess.check_output(['git', *args], cwd=root, text=True).strip()


def identity(root: Path) -> dict:
    return {'candidate': git(root, 'rev-parse', 'HEAD'), 'tree': git(root, 'rev-parse', 'HEAD^{tree}')}


def make_plan(root: Path, event: dict, event_name: str, ref: str, base: str | None = None,
              provider: str = 'local') -> dict:
    if event_name not in EVENTS:
        raise ValueError(f'unsupported CI event: {event_name}')
    full = event_name in {'schedule', 'manual', 'tag'} or ref.startswith('refs/tags/mainframe-env-v')
    reason = 'full-tier' if full else 'changed-paths'
    paths: list[str] = []
    if not full:
        if base is None:
            # The legacy event shape remains accepted so historical callers can
            # still reproduce their plans. Jenkins passes an explicit base.
            base = (event.get('pull_request', {}).get('base', {}).get('sha')
                    if event_name == 'pull_request' else event.get('before'))
        if base and SHA.fullmatch(base) and base != '0' * 40:
            try:
                output = subprocess.check_output(['git', 'diff', '--name-only', '--no-renames', '-z', base, 'HEAD'], cwd=root)
                paths = [x.decode('utf-8') for x in output.split(b'\0') if x]
                selected = obligations(paths)
            except (subprocess.CalledProcessError, UnicodeError):
                selected = sorted(ALL); reason = 'unavailable-diff-select-all'
        else:
            selected = sorted(ALL); reason = 'missing-base-select-all'
    else:
        selected = sorted(ALL)
    build = bool(selected)
    merge_push = event_name == 'push' and event.get('merge_commit', False)
    msrv = build and (full or not merge_push)
    # Dependency and license policy is intentionally unconditional: prose-only
    # pull requests, scheduled/full runs, and release tags all remain blocked by
    # a red locked dependency policy.
    gates = list(POLICY)
    if build:
        gates.extend(PRIMARY)
    if 'architecture' in selected and not full: gates.append('architecture-fast')
    if 'evidence' in selected and not full: gates.append('evidence-fast')
    if 'mutation' in selected: gates.append('mutation')
    if build and event_name != 'pull_request': gates.extend(['targets', 'documentation'])
    if full: gates.extend(gate for gate in FULL if gate not in gates)
    return {'schema_version': 'mainframe-env.ci-plan@1', **identity(root), 'provider': provider,
            'event': event_name, 'ref': ref, 'base': base, 'full': full,
            'build': build, 'msrv': msrv, 'store': 'store' in selected, 'mutation': 'mutation' in selected,
            'architecture': 'architecture' in selected, 'evidence': 'evidence' in selected,
            'obligations': selected, 'paths': paths, 'selection_reason': reason, 'primary_gates': gates,
            'licensed_credit': 0}


def write_json(path: Path, value: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temp = path.with_suffix(path.suffix + '.tmp')
    temp.write_text(json.dumps(value, indent=2, sort_keys=True) + '\n')
    temp.replace(path)


def record(root: Path, output: Path, gate: str, command: list[str], expect_tests: bool = False) -> int:
    if not re.fullmatch(r'[a-z][a-z0-9-]*', gate) or not command:
        raise ValueError('a gate name and command are required')
    output.mkdir(parents=True, exist_ok=True)
    before = identity(root)
    started = time.monotonic()
    tests = 0
    log = output / (gate + '.log')
    code = 127
    error = None
    try:
        with log.open('wb') as file:
            process = subprocess.Popen(command, cwd=root, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
            assert process.stdout is not None
            for line in process.stdout:
                file.write(line)
                sys.stdout.buffer.write(line); sys.stdout.buffer.flush()
                clean = re.sub(rb'\x1b\[[0-9;]*m', b'', line)
                match = re.search(rb'test result: ok\. (\d+) passed;', clean)
                if match: tests += int(match.group(1))
            process.stdout.close()
            code = process.wait()
    except OSError as problem:
        error = str(problem)
        log.write_text(error + '\n')
    unchanged = identity(root) == before and not subprocess.check_output(['git', 'status', '--porcelain', '--untracked-files=no'], cwd=root).strip()
    passed = code == 0 and unchanged and (not expect_tests or tests > 0)
    receipt = {'schema_version': 'mainframe-env.ci-command@1', **before, 'gate': gate, 'command': command,
               'exit_code': code, 'status': 'passed' if passed else 'failed', 'error': error,
               'duration_seconds': round(time.monotonic() - started, 6), 'observed_passed_tests': tests,
               'requires_nonempty_tests': expect_tests, 'candidate_unchanged': unchanged,
               'log_sha256': hashlib.sha256(log.read_bytes()).hexdigest(), 'full_assurance_credit': False,
               'licensed_credit': 0}
    jenkins = bool(os.environ.get('JENKINS_URL') or os.environ.get('JENKINS_HOME'))
    receipt['runner'] = {
        'ci': 'jenkins' if jenkins else 'local',
        'os': platform.system(),
        'arch': platform.machine(),
        'node_name': os.environ.get('NODE_NAME'),
        'job_name': os.environ.get('JOB_NAME'),
        'build_number': os.environ.get('BUILD_NUMBER'),
        'build_url': os.environ.get('BUILD_URL'),
    }
    for name, argv in [('rustc',['rustc','-Vv']),('cargo',['cargo','-V'])]:
        receipt[name] = subprocess.check_output(argv, cwd=root, text=True).strip() if shutil.which(argv[0]) else None
    write_json(output / (gate + '.json'), receipt)
    return 0 if passed else (code or 1)


def summarize(plan: dict, directory: Path, gates: list[str]) -> dict:
    results = {}
    for gate in gates:
        path = directory / (gate + '.json')
        receipt = json.loads(path.read_text()) if path.is_file() else None
        if receipt and any(receipt.get(key) != plan[key] for key in ('candidate', 'tree')):
            results[gate] = {'status': 'wrong-candidate'}
        else:
            results[gate] = receipt or {'status': 'not-run'}
    selected_commands_passed = (all(value['status'] == 'passed' for value in results.values())
                                if gates else not plan.get('build', True))
    return {'schema_version': 'mainframe-env.ci-summary@1', 'candidate': plan['candidate'], 'tree': plan['tree'],
            'tier': 'full' if plan['full'] else 'pr', 'checks': results,
            'selected_commands_passed': selected_commands_passed,
            'unselected_full_gates': [gate for gate in FULL if gate not in gates],
            'release_acceptance': False, 'licensed_credit': 0,
            'note': 'This command receipt set is not aggregate Jenkins, backend/MSRV evidence, or release acceptance.'}


def jenkins_context(root: Path, environ: dict[str, str], requested_event: str = 'auto',
                    requested_ref: str | None = None,
                    requested_base: str | None = None) -> dict[str, str | None]:
    """Resolve a Jenkins build to the provider-neutral plan inputs."""
    event = requested_event
    if event == 'auto':
        event = environ.get('MAINFRAME_ENV_CI_EVENT', '')
    if event == 'auto' or not event:
        branch = environ.get('TAG_NAME') or environ.get('BRANCH_NAME', '')
        if environ.get('CHANGE_ID'):
            event = 'pull_request'
        elif branch.startswith('mainframe-env-v'):
            event = 'tag'
        elif environ.get('BUILD_CAUSE') == 'TIMERTRIGGER':
            event = 'schedule'
        elif environ.get('JENKINS_URL') or environ.get('JENKINS_HOME'):
            event = 'push'
        else:
            event = 'local'
    if event not in EVENTS:
        raise ValueError(f'unsupported CI event: {event}')

    ref = requested_ref or environ.get('MAINFRAME_ENV_CI_REF')
    if not ref:
        if event == 'pull_request':
            change_id = environ.get('CHANGE_ID', '')
            ref = f'refs/pull/{change_id}/merge' if change_id else 'refs/pull/unknown/merge'
        elif event == 'tag':
            tag = environ.get('TAG_NAME') or environ.get('BRANCH_NAME', '')
            ref = f'refs/tags/{tag}'
        else:
            branch = environ.get('BRANCH_NAME') or 'local'
            ref = f'refs/heads/{branch}'

    base = requested_base or environ.get('MAINFRAME_ENV_CI_BASE') or None
    if base == '':
        base = None
    if base is not None and not SHA.fullmatch(base):
        raise ValueError('CI comparison base must be a full lowercase commit SHA')
    provider = 'jenkins' if environ.get('JENKINS_URL') or environ.get('JENKINS_HOME') else 'local'
    return {'event': event, 'ref': ref, 'base': base, 'provider': provider}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=Path(__file__).resolve().parents[1])
    sub = parser.add_subparsers(dest='mode', required=True)
    p = sub.add_parser('plan'); p.add_argument('--output', type=Path, required=True)
    p.add_argument('--event', choices=['auto', *sorted(EVENTS)], default='auto')
    p.add_argument('--ref'); p.add_argument('--base')
    p.add_argument('--merge-commit', action='store_true')
    p = sub.add_parser('record'); p.add_argument('--output', type=Path, required=True); p.add_argument('--gate', required=True); p.add_argument('--expect-tests', action='store_true'); p.add_argument('command', nargs=argparse.REMAINDER)
    p = sub.add_parser('summary'); p.add_argument('--plan', type=Path, required=True); p.add_argument('--directory', type=Path, required=True); p.add_argument('--output', type=Path, required=True); p.add_argument('--gates', nargs='*')
    args = parser.parse_args(); root = args.root.resolve()
    if args.mode == 'plan':
        context = jenkins_context(root, os.environ, args.event, args.ref, args.base)
        event = {'merge_commit': args.merge_commit}
        plan = make_plan(root, event, str(context['event']), str(context['ref']),
                         context['base'], str(context['provider']))
        write_json(args.output, plan)
        print(json.dumps(plan, sort_keys=True)); return 0
    if args.mode == 'record':
        command = args.command[1:] if args.command[:1] == ['--'] else args.command
        return record(root, args.output, args.gate, command, args.expect_tests)
    plan = json.loads(args.plan.read_text())
    summary = summarize(plan, args.directory, args.gates if args.gates is not None else plan['primary_gates'])
    write_json(args.output, summary); print(json.dumps(summary, sort_keys=True))
    return 0 if summary['selected_commands_passed'] else 1


if __name__ == '__main__':
    try: raise SystemExit(main())
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        raise SystemExit(f'CI assurance failed closed: {error}')
