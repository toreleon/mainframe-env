"""Linux resource witness for one existing supervised command session.

The outer ci_assurance owner supplies every deadline, signal and group census.
This dedicated child survives TERM/INT to reap only its kernel-owned children.
Group SIGKILL can still kill this wrapper; that case must retain unknown closure.
No global/root subreaper or escaped-session signalling is introduced.
"""
import ctypes
import errno
import json
import os
from pathlib import Path
import signal
import stat
import sys
import time


PR_SET_CHILD_SUBREAPER = 36
PR_GET_CHILD_SUBREAPER = 37
MAX_ADOPTED_CHILDREN = 65536
CANCEL_SIGNALS = (signal.SIGINT, signal.SIGTERM)


def _require(condition, message):
    if not condition:
        raise ValueError(message)


def _subreaper(enabled=None):
    if sys.platform != 'linux':
        raise ValueError('owned resource reaping requires Linux')
    libc = ctypes.CDLL(None, use_errno=True)
    if not hasattr(libc, 'prctl'):
        raise OSError('Linux child-subreaper control unavailable')
    libc.prctl.argtypes = (ctypes.c_int, ctypes.c_ulong, ctypes.c_ulong,
                          ctypes.c_ulong, ctypes.c_ulong)
    libc.prctl.restype = ctypes.c_int
    if enabled is not None and libc.prctl(PR_SET_CHILD_SUBREAPER, int(enabled), 0, 0, 0):
        raise OSError(ctypes.get_errno(), 'Linux child-subreaper control failed')
    actual = ctypes.c_int()
    if libc.prctl(PR_GET_CHILD_SUBREAPER, ctypes.addressof(actual), 0, 0, 0):
        raise OSError(ctypes.get_errno(), 'Linux child-subreaper query failed')
    _require(actual.value in (0, 1), 'Linux child-subreaper value differs')
    if enabled is not None:
        _require(actual.value == int(enabled), 'Linux child-subreaper admission differs')
    return actual.value


def _wait_primary(pid):
    while True:
        try:
            waited, status, usage = os.wait4(pid, 0)
            break
        except InterruptedError:
            continue
    _require(waited == pid, 'primary wait4 identity differs')
    return status, usage


def _reap_adopted(primary):
    rows = []
    while True:
        try:
            observed = os.waitid(os.P_ALL, 0, os.WEXITED | os.WNOWAIT)
        except InterruptedError:
            continue
        except ChildProcessError as problem:
            if problem.errno != errno.ECHILD:
                raise
            return rows
        _require(observed is not None and observed.si_pid > 1
                 and observed.si_pid not in (os.getpid(), primary),
                 'adopted child observation identity differs')
        _require(observed.si_code in (os.CLD_EXITED, os.CLD_KILLED, os.CLD_DUMPED),
                 'adopted child exit observation differs')
        _require(len(rows) < MAX_ADOPTED_CHILDREN, 'adopted child inventory exceeds 65536')
        # waitid names only an actual current child of this dedicated wrapper.
        # It is never an arbitrary PID from procfs or a released numeric PGID.
        status, usage = _wait_primary(observed.si_pid)
        expected = observed.si_status if observed.si_code == os.CLD_EXITED else -observed.si_status
        _require(os.waitstatus_to_exitcode(status) == expected,
                 'adopted child wait4 differs from retained exit observation')
        rows.append({
            'child_pid': observed.si_pid,
            'wait_status': status,
            'child_exit_code': os.waitstatus_to_exitcode(status),
            'user_seconds': usage.ru_utime,
            'system_seconds': usage.ru_stime,
            'max_process_rss_kib': usage.ru_maxrss,
        })


def _parent_identity(path):
    current = path.lstat()
    _require(stat.S_ISDIR(current.st_mode) and path.resolve() == path,
             'resource parent is not a plain canonical directory')
    return current.st_dev, current.st_ino


def _absent(path):
    try:
        path.lstat()
    except FileNotFoundError:
        return
    raise ValueError('resource destination already exists')


def _save_resource(destination, owner, row):
    raw = (json.dumps(row, indent=2) + '\n').encode()
    _require(_parent_identity(destination.parent) == owner, 'resource parent changed')
    _absent(destination)
    fd = os.open(destination, os.O_WRONLY | os.O_CREAT | os.O_EXCL
                 | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600)
    with os.fdopen(fd, 'wb') as stream:
        opened = os.fstat(stream.fileno())
        _require(_parent_identity(destination.parent) == owner, 'resource parent changed')
        actual = destination.lstat()
        _require((actual.st_dev, actual.st_ino) == (opened.st_dev, opened.st_ino)
                 and stat.S_ISREG(actual.st_mode) and actual.st_nlink == 1
                 and actual.st_uid == os.getuid() and stat.S_IMODE(actual.st_mode) == 0o600,
                 'resource destination identity differs')
        stream.write(raw)
        stream.flush()
        os.fsync(stream.fileno())
        _require(_parent_identity(destination.parent) == owner, 'resource parent changed')
        actual = destination.lstat()
        final = os.fstat(stream.fileno())
        _require((actual.st_dev, actual.st_ino, actual.st_size)
                 == (opened.st_dev, opened.st_ino, final.st_size)
                 and actual.st_size == len(raw) and actual.st_nlink == 1,
                 'retained resource identity differs')
        reader = os.open(destination, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC)
        try:
            read_owner = os.fstat(reader)
            _require((read_owner.st_dev, read_owner.st_ino, read_owner.st_size)
                     == (opened.st_dev, opened.st_ino, len(raw)),
                     'retained resource reader identity differs')
            chunks = []
            count = 0
            while True:
                _require(_parent_identity(destination.parent) == owner, 'resource parent changed')
                data = os.read(reader, 65536)
                if not data:
                    break
                count += len(data)
                _require(count <= len(raw), 'resource reader exceeds actual witness')
                chunks.append(data)
            actual = destination.lstat()
            _require(_parent_identity(destination.parent) == owner, 'resource parent changed')
            _require((actual.st_dev, actual.st_ino, actual.st_size)
                     == (opened.st_dev, opened.st_ino, len(raw))
                     and actual.st_nlink == 1 and stat.S_IMODE(actual.st_mode) == 0o600
                     and actual.st_uid == os.getuid() and b''.join(chunks) == raw,
                     'retained resource bytes differ from actual waits')
        finally:
            os.close(reader)


def run(destination, command):
    _require(command and command[0]
             and all(isinstance(x, str) and '\x00' not in x for x in command),
             'actual command arguments required')
    _require(os.getpid() > 1 and os.getpgrp() == os.getpid()
             and os.getsid(0) == os.getpid(), 'resource wrapper requires its born session')
    destination = Path(destination).absolute()
    owner = _parent_identity(destination.parent)
    _absent(destination)
    started = time.monotonic()
    _subreaper(True)
    deferred = set()
    finalizing = False

    def cancel(signum, frame):
        deferred.add(signum)
        # During wait/reap the dedicated wrapper must remain available. After
        # those waits, cancellation must refuse any already-written snapshot.
        if finalizing:
            raise InterruptedError(errno.EINTR, 'resource wrapper cancelled during finalization')

    previous = {sig: signal.getsignal(sig) for sig in CANCEL_SIGNALS}
    for sig in CANCEL_SIGNALS:
        signal.signal(sig, cancel)
    try:
        _require(not deferred, 'resource wrapper cancelled before birth')
        child = os.fork()
        if child == 0:
            try:
                for sig in CANCEL_SIGNALS:
                    signal.signal(sig, signal.SIG_DFL)
                _subreaper(False)
                if deferred:
                    os._exit(128 + min(deferred))
                os.execvp(command[0], command)
            except Exception as problem:
                number = problem.errno if isinstance(problem, OSError) else None
                os.write(2, ('resource child exec failed, errno=' + str(number) + '\n').encode())
            os._exit(127)
        status, usage = _wait_primary(child)
        adopted = _reap_adopted(child)
        finalizing = True
        exit_code = os.waitstatus_to_exitcode(status)
        wrapper_code = exit_code if exit_code >= 0 else 128 - exit_code
        if deferred and wrapper_code == 0:
            wrapper_code = 128 + min(deferred)
        _save_resource(destination, owner, {
            'schema_version': 'external.linux-command-resource@1',
            'child_pid': child, 'process_group': os.getpgrp(), 'wait_status': status,
            'child_exit_code': exit_code, 'duration_seconds': time.monotonic() - started,
            'user_seconds': usage.ru_utime, 'system_seconds': usage.ru_stime,
            'max_process_rss_kib': usage.ru_maxrss,
            'qualification': 'Actual primary Linux wait4 accounting; maximum per-process RSS, '
                             'not aggregate group peak. Adopted waits are separate. The existing '
                             'outer owner alone supplies timeout, session, group signals and census.',
            'wrapper_exit_code': wrapper_code,
            'wrapper_deferred_signals': sorted(deferred),
            'wrapper_child_subreaper': True,
            'adopted_child_waits': adopted,
            'kernel_child_set_echild_observed': True,
            'group_closure_credit': 0,
        })
        return wrapper_code
    finally:
        for sig, handler in previous.items():
            signal.signal(sig, handler)


def main(argv=None):
    args = sys.argv[1:] if argv is None else argv
    if len(args) < 2:
        return 2
    try:
        return run(args[0], args[1:])
    except Exception as problem:
        number = problem.errno if isinstance(problem, OSError) else None
        os.write(2, ('owned resource witness failed: ' + type(problem).__name__
                     + ', errno=' + str(number) + '\n').encode())
        return 125


if __name__ == '__main__':
    raise SystemExit(main())
