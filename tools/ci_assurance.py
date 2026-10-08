"""Cost-aware CI selection and exact-candidate command receipts (standard library only)."""
from __future__ import annotations
import argparse
import hashlib
import json
import math
import os
import platform
import shutil
from pathlib import Path, PurePosixPath
import re
import selectors
import signal
import subprocess
import sys
import time
import threading

DOCS = 'docs'
ALL = frozenset({'architecture', 'evidence', 'runtime', 'compiler', 'store', 'mutation', DOCS})
BUILD_OBLIGATIONS = ALL - {DOCS}
SHARED = {
    'Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml', 'rustfmt.toml',
    'clippy.toml', 'deny.toml', 'Jenkinsfile',
}
PROSE = {'README.md', 'CHANGELOG.md', 'LICENSE', 'LICENSE.md', 'CONTRIBUTING.md', 'AGENTS.md'}
PRIMARY = ['fmt', 'spec', 'cobol', 'python-tooling-tests', 'api-docs', 'tests', 'clippy']
POLICY = ['supply-chain', 'cargo-deny', 'license-notices']
FULL = [
    'targets', 'documentation', 'docs', 'conformance', 'certification',
    'runtime-architecture', 'fuzz-smoke', 'fuzz-periodic',
    'model-check', 'coverage-baseline',
]
SHA = re.compile(r'[0-9a-f]{40}\Z')
EVENTS = frozenset({'local', 'push', 'pull_request', 'schedule', 'manual', 'tag'})


def obligations(paths: list[str]) -> list[str]:
    selected: set[str] = set()
    for path in paths:
        p = PurePosixPath(path)
        if not path or p.is_absolute() or '..' in p.parts or '\x00' in path or '\\' in path:
            raise ValueError('unsafe changed path')
        if path in PROSE or path.endswith('.md') or path in {
                'docs/documentation-registry.json',
                'docs/generated/documentation-manifest.json'}:
            selected.add(DOCS)
        elif path in SHARED or path.startswith(('.cargo/', '.github/', 'xtask/', 'tools/', 'crates/contracts/', 'crates/foundation/')):
            selected.update(ALL)
        elif path.startswith(('docs/contracts/', 'docs/architecture/', 'docs/decisions/', 'docs/compatibility/', 'docs/generated/', 'conformance/spec/')):
            selected.update(ALL)
        elif path.startswith(('conformance/',)):
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
    full = event_name in {'schedule', 'manual', 'tag'}
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
    build = bool(set(selected) & BUILD_OBLIGATIONS)
    docs = DOCS in selected
    merge_push = event_name == 'push' and event.get('merge_commit', False)
    msrv = build and (full or not merge_push)
    # Dependency and license policy is intentionally unconditional: prose-only
    # pull requests, scheduled/full runs, and tag builds all remain blocked by
    # a red locked dependency policy.
    gates = list(POLICY)
    if build:
        gates.extend(PRIMARY)
    if msrv:
        gates.append('msrv')
    if 'architecture' in selected and not full: gates.append('architecture-fast')
    if 'mutation' in selected: gates.append('mutation')
    if docs: gates.append('docs')
    if build and event_name != 'pull_request': gates.extend(['targets', 'documentation'])
    if full: gates.extend(gate for gate in FULL if gate not in gates)
    return {'schema_version': 'mainframe-env.ci-plan@1', **identity(root), 'provider': provider,
            'event': event_name, 'ref': ref, 'base': base, 'full': full,
            'build': build, 'msrv': msrv, 'store': 'store' in selected, 'mutation': 'mutation' in selected,
            'docs': docs,
            'architecture': 'architecture' in selected, 'evidence': 'evidence' in selected,
            'obligations': selected, 'paths': paths, 'selection_reason': reason, 'primary_gates': gates,
            'licensed_credit': 0}


def write_json(path: Path, value: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temp = path.with_suffix(path.suffix + '.tmp')
    temp.write_text(json.dumps(value, indent=2, sort_keys=True) + '\n')
    temp.replace(path)


def passed_test_summary(line: bytes) -> int | None:
    """Count complete successful summaries; ignored/skipped tests earn no floor credit."""
    if b'test result:' not in line:
        return None
    rust = re.fullmatch(
        rb'test result: (ok|FAILED)\. ([0-9]+) passed; ([0-9]+) failed; '
        rb'[0-9]+ ignored; [0-9]+ measured; [0-9]+ filtered out; '
        rb'finished in [0-9]+(?:\.[0-9]+)?s', line.strip())
    if rust and rust[1] == b'ok' and int(rust[3]) == 0:
        return int(rust[2])
    tooling = re.fullmatch(
        rb'tooling test result: ok\. ([0-9]+) executed; ([0-9]+) skipped;'
        rb'(?: [0-9]+ python files; [0-9]+ shell test files; '
        rb'[0-9]+ shell syntax checks)?', line.strip())
    if tooling and int(tooling[2]) <= int(tooling[1]):
        return int(tooling[1]) - int(tooling[2])
    raise ValueError('minimum test floor requires complete successful test summaries')


def _validate_limits(timeout_seconds, max_output_bytes) -> None:
    """A bounded invocation must have a representable, positive Linux deadline."""
    if timeout_seconds is None:
        if max_output_bytes is not None:
            raise ValueError('an output limit requires a deadline')
        return
    if os.name != 'posix':
        raise ValueError('bounded commands require POSIX process groups')
    if sys.platform != 'linux' or not callable(getattr(os, 'waitid', None)) or not all(
            hasattr(os, name) for name in ('P_PID', 'WEXITED', 'WNOHANG', 'WNOWAIT')):
        raise ValueError('bounded commands require Linux non-reaping waitid and procfs')
    if signal.getsignal(signal.SIGCHLD) != signal.SIG_DFL:
        raise ValueError('bounded commands require exclusive child-wait ownership and default SIGCHLD')
    try:
        # The caller cannot be its own child. ECHILD validates syscall/flag
        # availability without creating or reaping a prerequisite process.
        os.waitid(os.P_PID, os.getpid(), os.WEXITED | os.WNOHANG | os.WNOWAIT)
    except ChildProcessError:
        pass
    except OSError as problem:
        raise ValueError('Linux non-reaping waitid is unavailable') from problem
    else:
        raise ValueError('unexpected non-reaping waitid capability result')
    try:
        fields = Path('/proc/self/stat').read_bytes().rsplit(b')', 1)[1].split()
        pid = int(Path('/proc/self/stat').read_bytes().split(b'(', 1)[0])
        if pid != os.getpid() or int(fields[2]) != os.getpgrp():
            raise ValueError('procfs must use the caller PID namespace')
    except (OSError, IndexError, ValueError) as problem:
        raise ValueError('bounded commands require readable matching Linux procfs') from problem
    try:
        valid = (type(timeout_seconds) in (int, float) and math.isfinite(timeout_seconds)
                 and 0 < timeout_seconds <= 86400)
    except OverflowError:
        valid = False
    if not valid:
        raise ValueError('deadline must be positive and finite, at most 86400 seconds')
    if max_output_bytes is not None and (
            type(max_output_bytes) is not int or not 0 < max_output_bytes <= sys.maxsize):
        raise ValueError('output limit must be a positive integer no larger than sys.maxsize')


def _linux_group_has_members(pgid: int) -> bool:
    """Conservatively inspect Linux group metadata, excluding its retained leader.

    Only numeric stat metadata is read. An incomplete, disappearing, unreadable
    or over-limit census fails instead of asserting an empty group. The unreaped
    leader fences PGID identity throughout this scan and any subsequent signal.
    Processes in a different group are outside this boundary. This is a census,
    not an atomic containment barrier against concurrent group changes/forking.
    """
    count = 0
    with os.scandir('/proc') as entries:
        for entry in entries:
            if not entry.name.isdecimal() or int(entry.name) == pgid:
                continue
            count += 1
            if count > 65536:
                raise OSError('owned group membership census exceeds 65536 processes')
            # Do not ignore vanished entries: a disappearing parent might have
            # forked an unlisted child. Unknown membership cannot earn success.
            raw = (Path(entry.path) / 'stat').read_bytes()
            try:
                group = int(raw.rsplit(b')', 1)[1].split()[2])
            except (IndexError, ValueError) as problem:
                raise OSError('invalid Linux process-group metadata') from problem
            if group == pgid:
                return True
    return False


def _run_owned(command: list[str], root: Path, on_output, *, timeout_seconds,
               max_output_bytes: int | None = None, separate_stderr: bool = False) -> tuple[int, str | None]:
    """Run one Linux session; retain its leader until every group operation ends.

    The callback must return promptly. Merged output retains pipe byte order;
    separate output retains stream labels, not a fabricated cross-stream order.
    Only the launched group is signalled. Descendants escaping that group are
    outside this primitive's containment boundary. An output ceiling is shared
    by both streams; admitted partial bytes survive failure and never imply pass.
    """
    _validate_limits(timeout_seconds, max_output_bytes)
    if timeout_seconds is None:
        raise ValueError('owned commands require a deadline')
    deadline = time.monotonic() + timeout_seconds
    process = None
    fence = False
    error = None
    code = 127
    count = 0
    observing = True
    overflowing = False
    cancelled = None
    pipes = []
    previous = {}
    selector = selectors.DefaultSelector()

    def fail(message):
        nonlocal error
        if error is None:
            error = message
        elif message not in error:
            error += '; ' + message

    def cancel(signum, frame):
        nonlocal cancelled
        # Do not interrupt Popen between fork and returning its owned handle.
        # Blocking these signals would also block them in the exec'd child.
        cancelled = signum

    def owned_group():
        # start_new_session makes this child's PID the only owned PGID. Never
        # substitute the caller's group or use a negative/broadcast PID.
        if not fence:
            raise OSError('owned process-group lifetime fence is unavailable')
        if process.pid <= 1 or process.pid in (os.getpid(), os.getpgrp()):
            raise OSError('refusing unsafe owned process group')
        return process.pid

    def observe_exit():
        nonlocal fence
        try:
            state = os.waitid(os.P_PID, process.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT)
        except ChildProcessError:
            fence = False
            raise
        if state is None or state.si_pid == 0:
            return None
        if state.si_pid != process.pid:
            raise OSError('non-reaping observation returned an unexpected child')
        if state.si_code == os.CLD_EXITED:
            return state.si_status
        if state.si_code in (os.CLD_KILLED, os.CLD_DUMPED):
            return -state.si_status
        raise OSError('non-reaping observation returned an unexpected child state')

    def group_has_members():
        return _linux_group_has_members(owned_group())

    def still_owned_work():
        # The zombie leader itself keeps killpg(0) successful. It is not a
        # leftover descendant and must not turn every zero exit into failure.
        return observe_exit() is None or group_has_members()

    def signal_group(sig):
        try:
            os.killpg(owned_group(), sig)
        except ProcessLookupError:
            pass

    def drain(wait):
        nonlocal count, observing, overflowing
        for key, _ in selector.select(wait):
            remaining = None if max_output_bytes is None else max_output_bytes - count
            try:
                size = min(65536, remaining + 1) if remaining is not None and not overflowing else 65536
                data = os.read(key.fd, size)
            except BlockingIOError:
                continue
            if not data:
                selector.unregister(key.fileobj)
                continue
            admitted = data if remaining is None else data[:remaining]
            count += len(admitted)
            if admitted and observing:
                try:
                    on_output(key.data, admitted)
                except BaseException:
                    observing = False
                    raise
            if remaining is not None and len(data) > remaining and not overflowing:
                overflowing = True
                raise ValueError('command output limit exceeded')

    try:
        if threading.current_thread() is threading.main_thread():
            for sig in (signal.SIGINT, signal.SIGTERM):
                previous[sig] = signal.getsignal(sig)
                signal.signal(sig, cancel)
        if cancelled is not None:
            raise InterruptedError(f'command cancelled by signal {cancelled}')
        process = subprocess.Popen(
            command, cwd=root, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
            stderr=subprocess.PIPE if separate_stderr else subprocess.STDOUT,
            start_new_session=True)
        fence = True
        pipes = [pipe for pipe in (process.stdout, process.stderr) if pipe is not None]
        owned_group()
        for stream, pipe in (('stdout', process.stdout), ('stderr', process.stderr)):
            if pipe is not None:
                os.set_blocking(pipe.fileno(), False)
                selector.register(pipe, selectors.EVENT_READ, stream)
        while True:
            if cancelled is not None:
                raise InterruptedError(f'command cancelled by signal {cancelled}')
            status = observe_exit()
            if status is not None and group_has_members():
                raise OSError('launcher exited with remaining owned process group')
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError('command deadline exceeded')
            if status is not None and not selector.get_map():
                break
            drain(min(remaining, 0.05))
    except BaseException as problem:
        fail(str(problem) or type(problem).__name__)
    finally:
        if cancelled is not None and error is None:
            fail(f'command cancelled by signal {cancelled}')
        if process is not None:
            try:
                if error is not None:
                    signal_group(signal.SIGTERM)
                    grace = time.monotonic() + 0.2
                    while time.monotonic() < grace:
                        if not still_owned_work():
                            break
                        try:
                            drain(min(0.02, max(0, grace - time.monotonic())))
                        except BaseException as problem:
                            fail(str(problem) or type(problem).__name__)
                    if still_owned_work():
                        signal_group(signal.SIGKILL)
            except BaseException as problem:
                fail('command cleanup failed: ' + (str(problem) or type(problem).__name__))
                if fence:
                    try:
                        signal_group(signal.SIGKILL)
                    except OSError as problem:
                        fail(str(problem))
            # Drain queued raw bytes even when signalling/observation failed.
            end = time.monotonic() + 0.1
            while selector.get_map() and time.monotonic() < end:
                try:
                    drain(min(0.02, max(0, end - time.monotonic())))
                except BaseException as problem:
                    fail(str(problem) or type(problem).__name__)
            if fence:
                try:
                    if group_has_members():
                        fail('owned process group remains after cleanup')
                        signal_group(signal.SIGKILL)
                except OSError as problem:
                    fail('owned group cleanup uncertain: ' + str(problem))
                    try:
                        signal_group(signal.SIGKILL)
                    except OSError as problem:
                        fail(str(problem))
            # Release group authority BEFORE the sole reaping attempt, even if
            # wait raises after reaping. No fallback may resurrect this PGID.
            fence = False
            try:
                code = process.wait(timeout=0.5)
            except BaseException as problem:
                code = 127
                fail('launcher wait failed: ' + (str(problem) or type(problem).__name__))
        if cancelled is not None and error is None:
            fail(f'command cancelled by signal {cancelled}')
        try:
            for pipe in pipes:
                try:
                    pipe.close()
                except OSError as problem:
                    fail('pipe cleanup failed: ' + str(problem))
            try:
                selector.close()
            except OSError as problem:
                fail('selector cleanup failed: ' + str(problem))
        finally:
            for sig, handler in previous.items():
                signal.signal(sig, handler)
    return code, error


def record(root: Path, output: Path, gate: str, command: list[str], expect_tests: bool = False,
           min_tests: int | None = None, *, timeout_seconds=None,
           max_output_bytes: int | None = None) -> int:
    if not re.fullmatch(r'[a-z][a-z0-9-]*', gate) or not command:
        raise ValueError('a gate name and command are required')
    if min_tests is not None and (type(min_tests) is not int or min_tests < 1):
        raise ValueError('minimum passed-test count must be a positive integer')
    _validate_limits(timeout_seconds, max_output_bytes)
    output.mkdir(parents=True, exist_ok=True)
    before = identity(root)
    clean_before = not subprocess.check_output(
        ['git', 'status', '--porcelain', '--untracked-files=all'], cwd=root).strip()
    started = time.monotonic()
    tests = 0
    log = output / (gate + '.log')
    code = 127
    error = None
    log_opened = False
    def count_line(line):
        nonlocal tests, error
        clean = re.sub(rb'\x1b\[[0-9;]*m', b'', line)
        if min_tests is not None:
            try:
                count = passed_test_summary(clean)
                if count is not None:
                    tests += count
            except ValueError as problem:
                error = str(problem)
        else:
            match = re.search(rb'test result: ok\. (\d+) passed;', clean)
            if match: tests += int(match.group(1))
            match = re.search(rb'tooling test result: ok\. (\d+) executed;', clean)
            if match: tests = int(match.group(1))

    try:
        if not clean_before:
            raise ValueError('CI candidate must be clean before execution, including untracked source')
        with log.open('wb') as file:
            log_opened = True
            if timeout_seconds is None:
                process = subprocess.Popen(command, cwd=root, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
                assert process.stdout is not None
                for line in process.stdout:
                    file.write(line)
                    sys.stdout.buffer.write(line); sys.stdout.buffer.flush()
                    count_line(line)
                process.stdout.close()
                code = process.wait()
            else:
                pending = bytearray()
                oversized = False

                def observe(stream, data):
                    nonlocal oversized, error
                    file.write(data)
                    sys.stdout.buffer.write(data); sys.stdout.buffer.flush()
                    # Bound summary framing memory independently of the raw log.
                    # An oversized line cannot contribute a summary prefix.
                    for part in data.splitlines(keepends=True):
                        if not oversized:
                            pending.extend(part)
                            if len(pending) > 65536:
                                pending.clear()
                                oversized = True
                                if min_tests is not None:
                                    error = 'minimum test floor requires summary lines within 65536 bytes'
                        if part.endswith(b'\n'):
                            if not oversized:
                                count_line(bytes(pending))
                            pending.clear()
                            oversized = False

                code, problem = _run_owned(command, root, observe, timeout_seconds=timeout_seconds,
                                           max_output_bytes=max_output_bytes)
                if problem is not None:
                    error = problem if error is None else error + '; ' + problem
                elif pending and not oversized:
                    count_line(bytes(pending))
    except (OSError, ValueError) as problem:
        error = str(problem)
        if isinstance(problem, ValueError):
            code = 2
        if timeout_seconds is None or not log_opened:
            log.write_text(error + '\n')
    unchanged = identity(root) == before and clean_before and not subprocess.check_output(
        ['git', 'status', '--porcelain', '--untracked-files=all'], cwd=root).strip()
    floor_met = min_tests is None or (error is None and tests >= min_tests)
    passed = code == 0 and error is None and unchanged and (not expect_tests or tests > 0) and floor_met
    receipt = {'schema_version': 'mainframe-env.ci-command@1', **before, 'gate': gate, 'command': command,
               'exit_code': code, 'status': 'passed' if passed else 'failed', 'error': error,
               'duration_seconds': round(time.monotonic() - started, 6), 'observed_passed_tests': tests,
               'requires_nonempty_tests': expect_tests, 'candidate_unchanged': unchanged,
               'log_sha256': hashlib.sha256(log.read_bytes()).hexdigest(), 'full_assurance_credit': False,
               'licensed_credit': 0}
    if min_tests is not None:
        receipt['minimum_passed_tests'] = min_tests
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
        if environ.get('CHANGE_ID'):
            event = 'pull_request'
        elif environ.get('TAG_NAME'):
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
    p = sub.add_parser('record'); p.add_argument('--output', type=Path, required=True); p.add_argument('--gate', required=True); p.add_argument('--expect-tests', action='store_true')
    p.add_argument('--min-tests', type=int, help='Minimum actual passed-test count; ignored/skipped tests do not count')
    p.add_argument('--timeout-seconds', type=float, help='Optional Linux deadline, positive and at most 86400 seconds')
    p.add_argument('--max-output-bytes', type=int, help='Optional combined byte ceiling; requires --timeout-seconds')
    p.add_argument('command', nargs=argparse.REMAINDER)
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
        return record(root, args.output, args.gate, command, args.expect_tests, args.min_tests,
                      timeout_seconds=args.timeout_seconds, max_output_bytes=args.max_output_bytes)
    plan = json.loads(args.plan.read_text())
    summary = summarize(plan, args.directory, args.gates if args.gates is not None else plan['primary_gates'])
    write_json(args.output, summary); print(json.dumps(summary, sort_keys=True))
    return 0 if summary['selected_commands_passed'] else 1


if __name__ == '__main__':
    try: raise SystemExit(main())
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        raise SystemExit(f'CI assurance failed closed: {error}')
