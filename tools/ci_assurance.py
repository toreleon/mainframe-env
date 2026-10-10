"""Cost-aware CI selection and exact-candidate command receipts (standard library only)."""
from __future__ import annotations
import argparse
from contextvars import ContextVar
import errno
import hashlib
import importlib.util
import json
import math
import os
import platform
import shutil
from pathlib import Path, PurePosixPath
import re
import selectors
import signal
import stat
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


class _LinuxCensusVanished(OSError):
    """One numeric stat vanished; its entire census is unusable."""


_LINUX_CENSUS_UNTIL = ContextVar('_linux_census_until', default=None)
_LINUX_CENSUS_TRACE = ContextVar('_linux_census_trace', default=None)
_LINUX_CENSUS_OWNER = ContextVar('_linux_census_owner', default=None)
_LINUX_CENSUS_DIAGNOSTICS = ContextVar('_linux_census_diagnostics', default=None)


def _census_cpu_time():
    try:
        return time.thread_time()
    except Exception:
        # Optional diagnostics never replace the existing census result/error.
        return None


def _retain_census_failure(trace, problem, collector):
    try:
        finished = time.monotonic()
    except Exception:
        finished = None
    cpu_finished = _census_cpu_time()
    started = trace['started_monotonic_seconds']
    cpu_started = trace.pop('cpu_started_seconds')
    trace.update({
        'finished_monotonic_seconds': finished,
        'elapsed_seconds': None if finished is None else finished - started,
        'thread_cpu_seconds': None if cpu_started is None or cpu_finished is None
            else cpu_finished - cpu_started,
        'error_type': type(problem).__name__[:80],
        'errno': problem.errno if type(problem.errno) is int else None,
        'retry_count': max(0, len(trace['numeric_entries_per_scan']) - 1),
    })
    # These are failure diagnostics, never membership evidence or closure credit.
    # Bound retained rows independently from the unchanged census/command limits.
    if len(collector['observations']) < 8:
        collector['observations'].append(trace)
    else:
        collector['omitted_observations'] += 1


def _check_linux_census_deadline(until) -> None:
    if until is not None and time.monotonic() >= until:
        raise OSError('owned group membership census observation deadline exceeded')


def _linux_stat_bytes(path: str) -> bytes:
    # scandir already supplies the absolute path. Avoid rebuilding pathlib
    # objects for every PID within the fixed census allowance; still read the
    # entire stat and propagate open/read/close failures.
    trace = _LINUX_CENSUS_TRACE.get()
    if trace is not None:
        trace['stage'] = 'stat_open'
    with open(path, 'rb') as stream:
        if trace is not None:
            trace['stage'] = 'stat_read'
        raw = stream.read()
        if trace is not None:
            trace['stage'] = 'stat_close'
    return raw


def _linux_group_has_members(pgid: int) -> bool:
    """Conservatively inspect Linux group metadata, excluding its retained leader.

    Only numeric stat metadata is read. An incomplete, disappearing, unreadable
    or over-limit census fails instead of asserting an empty group. The unreaped
    leader fences PGID identity throughout this scan and any subsequent signal.
    Processes in a different group are outside this boundary. This is a census,
    not an atomic containment barrier against concurrent group changes/forking.
    """
    until = _LINUX_CENSUS_UNTIL.get()
    trace = _LINUX_CENSUS_TRACE.get()
    count = 0
    if trace is not None:
        trace['stage'] = 'scandir_open'
    with os.scandir('/proc') as entries:
        if trace is not None:
            trace['stage'] = 'scandir_iterate'
        for entry in entries:
            if trace is not None:
                trace['entries_seen'] += 1
            if not entry.name.isdecimal() or int(entry.name) == pgid:
                continue
            if trace is not None:
                trace['stage'] = 'numeric_entry_deadline'
            _check_linux_census_deadline(until)
            count += 1
            if trace is not None:
                trace['numeric_entries_per_scan'][-1] = count
                trace['last_numeric_pid'] = entry.name[:20]
                trace['stage'] = 'numeric_entry_limit'
            if count > 65536:
                raise OSError('owned group membership census exceeds 65536 processes')
            # Do not ignore vanished entries: a disappearing parent might have
            # forked an unlisted child. Unknown membership cannot earn success.
            if trace is not None:
                trace['stage'] = 'stat_read_deadline'
            _check_linux_census_deadline(until)
            try:
                if trace is not None:
                    trace['stage'] = 'stat_read_call'
                    trace['stat_reads_started'] += 1
                raw = _linux_stat_bytes(entry.path + '/stat')
                if trace is not None:
                    trace['stat_reads_completed'] += 1
            except OSError as problem:
                if problem.errno in (errno.ENOENT, errno.ESRCH):
                    raise _LinuxCensusVanished(
                        problem.errno, problem.strerror, problem.filename) from problem
                raise
            try:
                if trace is not None:
                    trace['stage'] = 'stat_parse'
                group = int(raw.rsplit(b')', 1)[1].split()[2])
            except (IndexError, ValueError) as problem:
                raise OSError('invalid Linux process-group metadata') from problem
            if group == pgid:
                if trace is not None:
                    trace['member_observed'] = True
                    trace['stage'] = 'positive_scandir_close'
                return True
            if trace is not None:
                trace['stage'] = 'scandir_iterate'
        if trace is not None:
            trace['stage'] = 'scandir_close'
    if trace is not None:
        trace['stage'] = 'scan_complete_deadline'
    _check_linux_census_deadline(until)
    return False


def _linux_group_observation(pgid: int, deadline: float) -> bool:
    """Accept only complete negative censuses within one fixed local allowance.

    The caller must retain the leader throughout every observation. A vanished
    numeric stat aborts the whole scan; at most two fresh scans may follow. This
    does not make procfs atomic or preempt a blocked metadata syscall.
    """
    started = time.monotonic()
    until = min(deadline, started + 0.05)
    token = _LINUX_CENSUS_UNTIL.set(until)
    collector = _LINUX_CENSUS_DIAGNOSTICS.get()
    trace = None
    if collector is not None:
        owner = _LINUX_CENSUS_OWNER.get()
        trace = {
            'stage': 'observation_deadline',
            'started_monotonic_seconds': started,
            'deadline_monotonic_seconds': until,
            'cpu_started_seconds': _census_cpu_time(),
            'owned_leader_pid': None if owner is None else owner[0],
            'owned_pgid': None if owner is None else owner[1],
            'owner_phase': None if owner is None else owner[2],
            'leader_retained': owner is not None,
            'numeric_entries_per_scan': [], 'entries_seen': 0,
            'stat_reads_started': 0, 'stat_reads_completed': 0,
            'vanished_scans': 0, 'last_numeric_pid': None,
            'member_observed': False,
        }
    trace_token = _LINUX_CENSUS_TRACE.set(trace)
    try:
        for attempt in range(3):
            if trace is not None:
                trace['stage'] = 'observation_deadline'
            _check_linux_census_deadline(until)
            try:
                if trace is not None:
                    trace['numeric_entries_per_scan'].append(0)
                members = _linux_group_has_members(pgid)
            except _LinuxCensusVanished:
                if trace is not None:
                    trace['vanished_scans'] += 1
                if attempt == 2:
                    raise
                continue
            if members:
                # Positive evidence cannot be replaced by a later empty scan.
                return True
            if trace is not None:
                trace['stage'] = 'observation_complete_deadline'
            _check_linux_census_deadline(until)
            return False
        raise AssertionError('unreachable census observation state')
    except OSError as problem:
        if trace is not None:
            _retain_census_failure(trace, problem, collector)
        raise
    finally:
        _LINUX_CENSUS_TRACE.reset(trace_token)
        _LINUX_CENSUS_UNTIL.reset(token)


def _run_owned(command: list[str], root: Path, on_output, *, timeout_seconds,
               max_output_bytes: int | None = None, separate_stderr: bool = False,
               env=None, on_reaped=None, on_closed=None) -> tuple[int, str | None]:
    """Run one Linux session; retain its leader until every group operation ends.

    The callback must return promptly. Merged output retains pipe byte order;
    separate output retains stream labels, not a fabricated cross-stream order.
    Only the launched group is signalled. Descendants escaping that group are
    outside this primitive's containment boundary. An output ceiling is shared
    by both streams; admitted partial bytes survive failure and never imply pass.
    on_closed requires the non-reaped leader's exit, then a final negative group
    census and the sole successful leader wait; it does not imply test success.
    """
    child_options = {}
    if env is not None:
        if not isinstance(env, dict) or any(
                not isinstance(key, str) or not key or '=' in key or '\x00' in key
                or not isinstance(value, str) or '\x00' in value
                for key, value in env.items()):
            raise ValueError('child environment must contain valid string keys and values')
        child_options['env'] = dict(env)
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
    group_closed = False
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

    def group_has_members(observation_deadline, phase='monitor'):
        pgid = owned_group()
        if _LINUX_CENSUS_DIAGNOSTICS.get() is None:
            return _linux_group_observation(pgid, observation_deadline)
        token = _LINUX_CENSUS_OWNER.set((process.pid, pgid, phase))
        try:
            return _linux_group_observation(pgid, observation_deadline)
        finally:
            _LINUX_CENSUS_OWNER.reset(token)

    def still_owned_work(observation_deadline):
        # The zombie leader itself keeps killpg(0) successful. It is not a
        # leftover descendant and must not turn every zero exit into failure.
        return observe_exit() is None or group_has_members(observation_deadline, 'cleanup_grace')

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
            start_new_session=True, **child_options)
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
            if status is not None and group_has_members(deadline):
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
                        if not still_owned_work(grace):
                            break
                        try:
                            drain(min(0.02, max(0, grace - time.monotonic())))
                        except BaseException as problem:
                            fail(str(problem) or type(problem).__name__)
                    if still_owned_work(grace):
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
                    # A live leader can spawn after a census that excludes it.
                    # Establish its non-reaped exit before admitting emptiness.
                    if observe_exit() is None:
                        fail('launcher exit not established before final group census')
                        signal_group(signal.SIGKILL)
                    elif group_has_members(deadline if error is None else end, 'final_cleanup'):
                        fail('owned process group remains after cleanup')
                        signal_group(signal.SIGKILL)
                    else:
                        group_closed = True
                except OSError as problem:
                    fail('owned group cleanup uncertain: ' + str(problem))
                    try:
                        signal_group(signal.SIGKILL)
                    except OSError as problem:
                        fail(str(problem))
            # Release group authority BEFORE the sole reaping attempt, even if
            # wait raises after reaping. No fallback may resurrect this PGID.
            fence = False
            reaped = False
            try:
                code = process.wait(timeout=0.5)
                reaped = True
            except BaseException as problem:
                code = 127
                fail('launcher wait failed: ' + (str(problem) or type(problem).__name__))
            if reaped and on_reaped is not None:
                try:
                    on_reaped(code)
                except BaseException as problem:
                    fail('exit capture failed: ' + (str(problem) or type(problem).__name__))
            if group_closed and reaped and on_closed is not None:
                try:
                    on_closed(code)
                except BaseException as problem:
                    fail('closure capture failed: ' + (str(problem) or type(problem).__name__))
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
        # A reused output/gate must not retain a previous attempt's diagnostics.
        (output / (gate + '.census.json')).unlink(missing_ok=True)
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

                diagnostics = {'observations': [], 'omitted_observations': 0}
                token = _LINUX_CENSUS_DIAGNOSTICS.set(diagnostics)
                try:
                    code, problem = _run_owned(command, root, observe, timeout_seconds=timeout_seconds,
                                               max_output_bytes=max_output_bytes)
                finally:
                    _LINUX_CENSUS_DIAGNOSTICS.reset(token)
                if problem is not None:
                    error = problem if error is None else error + '; ' + problem
                elif pending and not oversized:
                    count_line(bytes(pending))
                if diagnostics['observations']:
                    try:
                        write_json(output / (gate + '.census.json'), {
                            'schema_version': 'mainframe-env.ci-census-diagnostic@1',
                            **before, 'gate': gate, **diagnostics,
                            'scope': 'Bounded failed procfs observations only; no closure or test credit.',
                        })
                    except OSError:
                        # The command already failed its census. Keep that exact
                        # error authoritative even if its sidecar cannot be saved.
                        try:
                            file.write(b'CI census diagnostic retention failed\n')
                        except OSError:
                            pass

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


PUBLIC_CLIENT_PROFILE = 'public-client-linux-x86_64'
_PUBLIC_CLIENT_SUPPLY = None
_PUBLIC_CLIENT_SEEDS = {
    'identity/passwd': b'agent:x:1000:1000:public-client:/client-home:/bin/false\n',
    'identity/group': b'agent:x:1000:\n',
    'identity/nsswitch.conf': b'passwd: files\ngroup: files\nhosts: files\n',
    'client-home/.zowe.env.json': b'{}\n',
    'plugins/plugins.json': b'{}\n',
    'client-home/settings/imperative.json': b'{"overrides":{"CredentialManager":false},"credentialManagerOptions":{}}\n',
    'input/public-client.jcl': b'//PBCLNT01 JOB CLASS=A\n//STEP1 EXEC PGM=IEFBR14\n',
}
_PUBLIC_CLIENT_DIRS = ('identity', 'client-home', 'plugins', 'cwd', 'input',
                       'client-home/settings', 'client-home/logs')
_PUBLIC_CLIENT_CAPTURES = ('stdout.bin', 'stderr.bin', 'child-exit.txt', 'supervision-error.txt')


def _public_client_supply_chain():
    """Load only the fixed sibling owner, also under importlib-based callers."""
    global _PUBLIC_CLIENT_SUPPLY
    if _PUBLIC_CLIENT_SUPPLY is None:
        spec = importlib.util.spec_from_file_location(
            '_ci_assurance_supply_chain', Path(__file__).resolve().with_name('supply_chain.py'))
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        _PUBLIC_CLIENT_SUPPLY = module
    return _PUBLIC_CLIENT_SUPPLY


def _public_client_decimal(value, maximum, *, minimum=0):
    if (not isinstance(value, str) or not re.fullmatch(r'0|[1-9][0-9]{0,7}', value)
            or not minimum <= int(value) <= maximum):
        raise ValueError('noncanonical or out-of-range public client scalar')
    return value


def _public_client_operation(action, port, job_id, file_id) -> list[str]:
    _public_client_decimal(port, 65535, minimum=1)
    shapes = {
        'submit': ('submit', 'local-file', '/input/public-client.jcl'),
        'bad-password': ('submit', 'local-file', '/input/public-client.jcl'),
        'owner-list': ('list', 'jobs', '--owner', 'IBMUSER', '--prefix', 'PBCLNT01'),
        'other-owner-list': ('list', 'jobs', '--owner', 'IBMUSER', '--prefix', 'PBCLNT01'),
        'status': ('view', 'job-status-by-jobid'),
        'other-status': ('view', 'job-status-by-jobid'),
        'files': ('list', 'spool-files-by-jobid'),
        'other-files': ('list', 'spool-files-by-jobid'),
        'content': ('view', 'spool-file-by-id'),
        'other-content': ('view', 'spool-file-by-id'),
    }
    if action not in shapes:
        raise ValueError('unknown public client action')
    needs_job = action in {'status', 'other-status', 'files', 'other-files', 'content', 'other-content'}
    needs_file = action in {'content', 'other-content'}
    if (job_id is not None) != needs_job or (file_id is not None) != needs_file:
        raise ValueError('public client action scalar applicability differs')
    operation = ['zos-jobs', *shapes[action]]
    if needs_job:
        if (not isinstance(job_id, str) or not re.fullmatch(r'JOB[0-9]{5,8}', job_id)
                or not 1 <= int(job_id[3:]) <= 99999999
                or job_id != f'JOB{int(job_id[3:]):05}'):
            raise ValueError('noncanonical public client job ID')
        operation.append(job_id)
    if needs_file:
        operation.append(_public_client_decimal(file_id, 63))
    user, password = ('OTHERUSR', 'OTHERPASS') if action.startswith('other-') else (
        'IBMUSER', 'WRONGPASS' if action == 'bad-password' else 'TESTPASS')
    return operation + ['--host', '127.0.0.1', '--port', port, '--protocol', 'http',
                        '--user', user, '--password', password, '--completion-timeout', '5',
                        '--establish-connection-timeout', '5', '--response-format-json']


def _public_client_identity() -> None:
    if (sys.platform != 'linux' or platform.machine() != 'x86_64'
            or os.getuid() != 1000 or os.geteuid() != 1000
            or os.getgid() != 1000 or os.getegid() != 1000
            or any(group != 1000 for group in os.getgroups())):
        raise ValueError('public client requires Linux x86_64 and reviewed uid/gid 1000')
    status = Path('/proc/self/status').read_text(encoding='ascii')
    for name in ('CapEff', 'CapPrm', 'CapAmb'):
        values = re.findall(rf'^{name}:\s*([0-9a-fA-F]+)$', status, re.MULTILINE)
        if len(values) != 1 or int(values[0], 16) != 0:
            raise ValueError('public client requires zero effective/permitted/ambient capabilities')


def _public_client_path(value) -> Path:
    raw = str(value)
    path = Path(raw)
    if (not path.is_absolute() or str(path) != raw or len(raw.encode('utf-8')) > 4096
            or '\\' in raw or any(ord(c) < 32 or 127 <= ord(c) <= 159 for c in raw)
            or path.resolve() != path):
        raise ValueError('public client path must be canonical, absolute and nonsymlink')
    return path


def _public_client_run_path(value, root: Path, files: dict, tree: Path) -> Path:
    path = _public_client_path(value)
    if path.exists() or path.is_symlink() or path in {Path('/'), Path.home().resolve()}:
        raise ValueError('public client run leaf must be new and exclusive')
    parent = path.parent
    metadata = parent.lstat()
    if (not stat.S_ISDIR(metadata.st_mode) or metadata.st_uid != os.getuid()
            or stat.S_IMODE(metadata.st_mode) & 0o022):
        raise ValueError('public client run parent must be private and owned')
    for authority in (root, tree, *files.values()):
        if path == authority or path in authority.parents or authority in path.parents:
            raise ValueError('public client run leaf overlaps source or input authority')
    return path


def _public_client_exclusive(path: Path):
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    try:
        os.fchmod(descriptor, 0o600)
        return os.fdopen(descriptor, 'wb', buffering=0)
    except BaseException:
        os.close(descriptor)
        raise


def _prepare_public_client_run(run_dir: Path) -> None:
    for name in _PUBLIC_CLIENT_DIRS:
        (run_dir / name).mkdir(mode=0o700)
        (run_dir / name).chmod(0o700)
    for name, data in _PUBLIC_CLIENT_SEEDS.items():
        with _public_client_exclusive(run_dir / name) as output:
            if output.write(data) != len(data):
                raise OSError('public client seed write incomplete')


def _public_client_check_state(run_dir: Path, lock_hash: str) -> None:
    """Inspect a finite retained tree; these caps are not live filesystem quotas."""
    regular = {**_PUBLIC_CLIENT_SEEDS, 'input-lock.sha256': (lock_hash + '\n').encode('ascii')}
    caps = {'stdout.bin': 65536, 'stderr.bin': 65536, 'child-exit.txt': 32,
            'supervision-error.txt': 4096,
            'client-home/logs/imperative.log': 1048576, 'client-home/logs/zowe.log': 1048576,
            'client-home/logs/imperative_debug.log': 1048576}
    expected = set(regular) | set(_PUBLIC_CLIENT_CAPTURES) | set(_PUBLIC_CLIENT_DIRS)
    found = set()
    pending = [run_dir]
    log_total = 0
    file_state = _public_client_supply_chain().file_state
    while pending:
        directory = pending.pop()
        before = directory.lstat()
        if (not stat.S_ISDIR(before.st_mode) or stat.S_IMODE(before.st_mode) != 0o700
                or before.st_uid != os.getuid()):
            raise ValueError('owned public client directory identity/mode differs')
        with os.scandir(directory) as entries:
            for entry in entries:
                path = Path(entry.path)
                name = path.relative_to(run_dir).as_posix()
                if (name not in expected and name not in caps) or name in found:
                    raise ValueError('unexpected public client tree membership')
                found.add(name)
                metadata = entry.stat(follow_symlinks=False)
                if name in _PUBLIC_CLIENT_DIRS:
                    if not stat.S_ISDIR(metadata.st_mode):
                        raise ValueError('public client directory type differs')
                    pending.append(path)
                    continue
                if (not stat.S_ISREG(metadata.st_mode) or metadata.st_uid != os.getuid()
                        or metadata.st_nlink != 1):
                    raise ValueError('public client file type/ownership differs')
                is_log = name.startswith('client-home/logs/')
                if (stat.S_IMODE(metadata.st_mode) & 0o7022 if is_log
                        else stat.S_IMODE(metadata.st_mode) != 0o600):
                    raise ValueError('public client file mode differs')
                maximum = len(regular[name]) if name in regular else caps[name]
                if metadata.st_size > maximum:
                    raise ValueError('public client file exceeds inspection cap')
                descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
                with os.fdopen(descriptor, 'rb') as source:
                    if file_state(os.fstat(source.fileno())) != file_state(metadata):
                        raise ValueError('public client file changed before read')
                    data = source.read(maximum + 1)
                    if len(data) > maximum or (name in regular and data != regular[name]):
                        raise ValueError('public client seed bytes or capture cap differs')
                    if (file_state(os.fstat(source.fileno())) != file_state(metadata)
                            or file_state(path.lstat()) != file_state(metadata)):
                        raise ValueError('public client file changed during inspection')
                if is_log: log_total += len(data)
        if file_state(directory.lstat()) != file_state(before):
            raise ValueError('public client directory changed during inspection')
    if not expected <= found or log_total > 2097152:
        raise ValueError('public client required membership/log total differs')


def _public_client_argv(validated: dict, run_dir: Path, suffix: list[str]) -> list[str]:
    files = validated['files']
    command = [str(files['bubblewrap']), '--unshare-user', '--uid', '1000', '--gid', '1000',
               '--unshare-pid', '--die-with-parent', '--new-session', '--cap-drop', 'ALL', '--clearenv',
               '--proc', '/proc', '--dev', '/dev', '--tmpfs', '/tmp', '--dir', '/etc',
               '--dir', '/opt/node/bin', '--dir', '/lib64', '--dir', '/lib/x86_64-linux-gnu']
    for role, destination in (
            ('node', '/opt/node/bin/node'),
            ('loader', '/lib/x86_64-linux-gnu/ld-linux-x86-64.so.2'),
            ('loader', '/lib64/ld-linux-x86-64.so.2'),
            ('libdl', '/lib/x86_64-linux-gnu/libdl.so.2'),
            ('libstdcxx', '/lib/x86_64-linux-gnu/libstdc++.so.6'),
            ('libm', '/lib/x86_64-linux-gnu/libm.so.6'),
            ('libgcc', '/lib/x86_64-linux-gnu/libgcc_s.so.1'),
            ('libpthread', '/lib/x86_64-linux-gnu/libpthread.so.0'),
            ('libc', '/lib/x86_64-linux-gnu/libc.so.6'),
            ('libnss_files', '/lib/x86_64-linux-gnu/libnss_files.so.2')):
        command += ['--ro-bind', str(files[role]), destination]
    for name, destination in (('identity/passwd', '/etc/passwd'), ('identity/group', '/etc/group'),
                              ('identity/nsswitch.conf', '/etc/nsswitch.conf'), ('input', '/input')):
        command += ['--ro-bind', str(run_dir / name), destination]
    command += ['--dir', '/opt/client/node_modules/@zowe', '--ro-bind',
                str(validated['tree'] / 'package'), '/opt/client/node_modules/@zowe/cli']
    for name, destination in (('client-home', '/client-home'), ('plugins', '/plugins'), ('cwd', '/work')):
        command += ['--bind', str(run_dir / name), destination]
    return command + ['--chdir', '/work', '--setenv', 'PATH', '/opt/node/bin',
                      '--setenv', 'TMPDIR', '/tmp', '--setenv', 'ZOWE_CLI_HOME', '/client-home',
                      '--setenv', 'ZOWE_CLI_PLUGINS_DIR', '/plugins', '/opt/node/bin/node',
                      '--no-addons', '--no-global-search-paths',
                      '/opt/client/node_modules/@zowe/cli/lib/main.js', *suffix]


def _public_client_diagnostic(message: str) -> bytes:
    # Never include fixed credentials, raw output or a human child argv log.
    for secret in ('TESTPASS', 'WRONGPASS', 'OTHERPASS', 'IBMUSER', 'OTHERUSR'):
        message = message.replace(secret, '[redacted]')
    escaped = ''.join(c if 32 <= ord(c) < 127 else ascii(c)[1:-1]
                      for c in message).encode('ascii', errors='backslashreplace')
    return escaped[:4095] + b'\n' if escaped else b''


def _public_client_lock_bytes(root: Path) -> bytes:
    supply = _public_client_supply_chain()
    path = root / 'tools/ci-inputs.lock.json'
    descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(descriptor, 'rb') as source:
        metadata = os.fstat(source.fileno())
        if not stat.S_ISREG(metadata.st_mode) or not 0 < metadata.st_size <= supply.MAX_JSON_BYTES:
            raise ValueError('public client lock size/type differs')
        data = source.read(supply.MAX_JSON_BYTES + 1)
        if (len(data) > supply.MAX_JSON_BYTES
                or supply.file_state(os.fstat(source.fileno())) != supply.file_state(metadata)
                or supply.file_state(path.lstat()) != supply.file_state(metadata)):
            raise ValueError('public client lock changed during bounded read')
        return data


def public_client_command(root: Path, *, profile_files, profile_tree, run_dir,
                          action, port, job_id=None, file_id=None, timeout_seconds) -> int:
    """Report only transport completion; the caller owns semantic assertions."""
    source_root = Path(__file__).resolve().parents[1]
    if root.resolve() != source_root:
        raise ValueError('public client root must match the executing source owner')
    suffix = _public_client_operation(action, port, job_id, file_id)
    try:
        valid_timeout = (type(timeout_seconds) in (int, float) and math.isfinite(timeout_seconds)
                         and 0 < timeout_seconds <= 10)
    except OverflowError:
        valid_timeout = False
    if not valid_timeout:
        raise ValueError('public client timeout must be finite, positive and at most 10 seconds')
    _public_client_identity()
    supply = _public_client_supply_chain()
    files = supply.profile_bindings(profile_files)
    tree = _public_client_path(profile_tree)
    run = _public_client_run_path(run_dir, source_root, files, tree)
    proof_context = None
    postcheck = None

    def inputs():
        nonlocal proof_context, postcheck
        before = _public_client_lock_bytes(source_root)
        lock = supply.validate_ci_lock(source_root)
        if lock['schema_version'] != 'mainframe-env.ci-input-lock@2':
            raise ValueError('public client requires the accepted optional @2 profile')
        if postcheck is None:
            context = supply._development_input_command(lock, PUBLIC_CLIENT_PROFILE, files, tree,
                                                         lock_bytes=before)
            validated, postcheck = context.__enter__()
            proof_context = context
        else:
            validated = postcheck(lock, PUBLIC_CLIENT_PROFILE, files, tree, lock_bytes=before)
        after = _public_client_lock_bytes(source_root)
        if before != after:
            raise ValueError('public client lock changed during input validation')
        return hashlib.sha256(before).hexdigest(), validated

    try:
        lock_hash, validated = inputs()
        # mkdir is the exclusive allocation boundary; never reuse or delete a leaf.
        run.mkdir(mode=0o700)
    except BaseException:
        if proof_context is not None:
            proof_context.__exit__(None, None, None)
        raise
    handles = {}
    failures = []
    reaped = False
    lengths = {'stdout': 0, 'stderr': 0}

    def output(stream, data):
        if stream not in lengths or not isinstance(data, bytes):
            raise ValueError('invalid public client raw output stream')
        remaining = min(65536 - lengths[stream], 131072 - sum(lengths.values()))
        admitted = data[:remaining]
        if admitted and handles[stream + '.bin'].write(admitted) != len(admitted):
            raise OSError('public client raw capture write incomplete')
        lengths[stream] += len(admitted)
        if len(data) > remaining:
            raise ValueError('public client ' + stream + ' output limit exceeded')

    def capture_exit(code):
        nonlocal reaped
        if reaped or type(code) is not int:
            raise ValueError('public client actual exit capture differs')
        data = (str(code) + '\n').encode('ascii')
        if len(data) > 32 or handles['child-exit.txt'].write(data) != len(data):
            raise OSError('public client actual exit write incomplete')
        reaped = True

    try:
        run.chmod(0o700)
        for name in _PUBLIC_CLIENT_CAPTURES:
            handles[name] = _public_client_exclusive(run / name)
        with _public_client_exclusive(run / 'input-lock.sha256') as lock_file:
            data = (lock_hash + '\n').encode('ascii')
            if lock_file.write(data) != len(data): raise OSError('public client lock capture incomplete')
        _prepare_public_client_run(run)
        _public_client_check_state(run, lock_hash)
        _, error = _run_owned(_public_client_argv(validated, run, suffix), run / 'cwd', output,
                             timeout_seconds=timeout_seconds, max_output_bytes=131072,
                             separate_stderr=True, env={}, on_reaped=capture_exit)
        if error: failures.append(error)
        if not reaped: failures.append('public client actual exit was not captured')
    except BaseException as problem:
        failures.append('public client setup/capture failed: ' + (str(problem) or type(problem).__name__))
    finally:
        try:
            after_hash, after_inputs = inputs()
            if after_hash != lock_hash or after_inputs != validated:
                raise ValueError('public client lock/input identities changed')
        except BaseException as problem:
            failures.append('public client input postcheck failed: ' + (str(problem) or type(problem).__name__))
        finally:
            if proof_context is not None:
                proof_context.__exit__(None, None, None)
        try:
            _public_client_check_state(run, lock_hash)
        except BaseException as problem:
            failures.append('public client state postcheck failed: ' + (str(problem) or type(problem).__name__))
        # Close captures before final diagnostics so close failures are retained.
        for name in tuple(handles):
            if name == 'supervision-error.txt': continue
            try: handles.pop(name).close()
            except OSError: failures.append('public client capture close failed')
        diagnostic = _public_client_diagnostic('; '.join(failures))
        try:
            error_file = handles.pop('supervision-error.txt', None)
            if error_file is None:
                error_file = _public_client_exclusive(run / 'supervision-error.txt')
            with error_file:
                if error_file.write(diagnostic) != len(diagnostic):
                    raise OSError('public client supervision diagnostic write incomplete')
        except OSError:
            failures.append('public client supervision diagnostic capture failed')
            print('public client supervision diagnostic capture failed', file=sys.stderr)
    return 1 if failures else 0


class _PublicClientScalar(argparse.Action):
    def __call__(self, parser, namespace, values, option_string=None):
        seen = getattr(namespace, '_public_client_seen', set())
        if self.dest in seen:
            parser.error('duplicate public client scalar option: ' + option_string)
        namespace._public_client_seen = seen | {self.dest}
        setattr(namespace, self.dest, values)


class _PublicClientArgumentParser(argparse.ArgumentParser):
    def __init__(self, *args, public_client_errors=False, **kwargs):
        self.public_client_errors = public_client_errors
        super().__init__(*args, **kwargs)

    def error(self, message):
        if self.public_client_errors:
            self.exit(2, _public_client_diagnostic('public client arguments refused').decode('ascii'))
        super().error(message)


def _public_client_cli_selected(arguments: list[str]) -> bool:
    # Argparse identifies only the mode after the existing global root option.
    # Optional root arity permits diagnostic routing even for a missing value;
    # the real parser still enforces its required value. Later tokens cannot
    # replace the first positional mode, including a record command remainder.
    selector = argparse.ArgumentParser(add_help=False, prog='ci_assurance.py')
    selector.add_argument('--root', nargs='?')
    selector.add_argument('mode', nargs='?')
    return selector.parse_known_args(arguments)[0].mode == 'public-client-command'


def main() -> int:
    public_errors = _public_client_cli_selected(sys.argv[1:])
    parser = _PublicClientArgumentParser(description=__doc__, public_client_errors=public_errors,
        allow_abbrev=not public_errors,
        **({'prog': 'ci_assurance.py'} if public_errors else {}))
    parser.add_argument('--root', type=Path, default=Path(__file__).resolve().parents[1],
                        **({'action': _PublicClientScalar} if public_errors else {}))
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
    p = sub.add_parser('public-client-command', allow_abbrev=False, public_client_errors=True,
                       prog='ci_assurance.py public-client-command')
    for name in ('development-profile', 'profile-tree', 'run-dir', 'action', 'port', 'timeout-seconds'):
        p.add_argument('--' + name, action=_PublicClientScalar, required=True,
                       **({'type': float} if name == 'timeout-seconds' else {}))
    for name in ('job-id', 'file-id'):
        p.add_argument('--' + name, action=_PublicClientScalar)
    p.add_argument('--profile-file', action='append', required=True)
    args = parser.parse_args()
    if args.mode == 'public-client-command':
        prefix = sys.argv[1:sys.argv.index('public-client-command')]
        roots = [token for token in prefix if token == '--root' or token.startswith('--root=')]
        if len(roots) > 1 or any(token.startswith('--') and token != '--root'
                                 and not token.startswith('--root=') for token in prefix):
            parser.error('unknown or duplicate public client root option')
        if args.development_profile != PUBLIC_CLIENT_PROFILE:
            parser.error('unknown public client development profile')
        try:
            root = args.root.resolve()
            return public_client_command(root, profile_files=args.profile_file,
                profile_tree=args.profile_tree, run_dir=args.run_dir, action=args.action,
                port=args.port, job_id=args.job_id, file_id=args.file_id,
                timeout_seconds=args.timeout_seconds)
        except (ValueError, OSError, _public_client_supply_chain().SupplyChainError):
            sys.stderr.write(_public_client_diagnostic('public client refused').decode('ascii'))
            return 1
    root = args.root.resolve()
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
