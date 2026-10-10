"""Linux source-complete corpus gate controls; fresh owned copy, no source mutation.

This private producer uses the existing bounded session/group supervisor. Its
same-group closure proof does not cover descendants that deliberately escape.
It never consumes prior receipts or grants application/license runtime credit.
"""
from __future__ import annotations
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import stat
import sys
import time

MAX_COPY_BYTES = 32 * 1024 * 1024
MAX_COPY_ENTRIES = 1024
MAX_OUTPUT_BYTES = 64 * 1024 * 1024
OUTER_SECONDS = 1800
COMMAND_SECONDS = 30
PROBE = 'carddemo::corpus_campaign_tests::corpus_campaign_child_probe'
REQUIREMENTS = [
    'CARDDEMO_CORPUS_DIR is required only for CardDemo gates',
    'commit, tree, cleanliness, license, content, runtime-archive, and file-count identities are checked',
    'dirty, missing, drifted, or unlicensed inputs fail',
    'no absolute local path enters evidence',
]


def digest(raw):
    return hashlib.sha256(raw).hexdigest()


def identity(path):
    value = path.lstat()
    if not stat.S_ISDIR(value.st_mode):
        raise ValueError('owned namespace is not a plain directory')
    return value.st_dev, value.st_ino


def scan(root):
    """No symlinks/special files; all tracked data and the actual Git metadata."""
    identity(root)
    entries = []; total = 0
    for current, directories, files in os.walk(root, followlinks=False):
        for name in sorted(directories + files):
            path = Path(current) / name; metadata = path.lstat()
            relative = path.relative_to(root).as_posix()
            if stat.S_ISDIR(metadata.st_mode):
                entry = {'path': relative, 'kind': 'directory'}
            elif stat.S_ISREG(metadata.st_mode):
                total += metadata.st_size
                if total > MAX_COPY_BYTES:
                    raise ValueError('source-complete copy byte bound exceeded')
                entry = {'path': relative, 'kind': 'file', 'bytes': metadata.st_size,
                         'sha256': digest(path.read_bytes()), 'mode': stat.S_IMODE(metadata.st_mode)}
            else:
                raise ValueError('source-complete copy has a link or special file')
            entries.append(entry)
            if len(entries) > MAX_COPY_ENTRIES:
                raise ValueError('source-complete copy entry bound exceeded')
    return sorted(entries, key=lambda row: row['path']), total


def copy_complete(source, target, entries, verify_target):
    for entry in entries:
        path = target / entry['path']
        if entry['kind'] == 'directory':
            verify_target()
            path.mkdir(parents=True, exist_ok=True)
        else:
            verify_target()
            path.parent.mkdir(parents=True, exist_ok=True)
            raw = (source / entry['path']).read_bytes()
            if len(raw) != entry['bytes'] or digest(raw) != entry['sha256']:
                raise ValueError('source changed during exclusive copy')
            verify_target()
            with path.open('xb') as stream:
                stream.write(raw)
            verify_target()
            path.chmod(entry['mode'])
    verify_target()
    copied, _ = scan(target)
    if copied != entries:
        raise ValueError('source-complete exclusive copy differs')


class Campaign:
    def __init__(self, args):
        self.args = args
        if sys.platform != 'linux':
            raise ValueError('corpus campaign requires the existing Linux supervisor contract')
        self.repo = args.repo.resolve(strict=True)
        self.source = args.corpus.resolve(strict=True)
        self.evidence = args.evidence.absolute()
        self.evidence.mkdir()  # No reuse of a previous attempt.
        self.evidence_id = identity(self.evidence)
        self.copy = self.evidence / 'owned-corpus'
        self.copy.mkdir()
        self.copy_id = identity(self.copy)
        self.start = time.monotonic()
        self.closed = True
        self.output_bytes = 0
        self.proven_requirements = []
        self.report = {'schema': 'private.carddemo.corpus-campaign@1', 'status': 'running',
                       'issue': 'CD-001', 'requirements': [], 'commands': [],
                       'namespace': {'dev': self.copy_id[0], 'inode': self.copy_id[1]},
                       'cleanup': {'status': 'pending'}, 'application_credit': 0,
                       'licensed_runtime_credit': 0,
                       'scope': 'selected real CLI dispatcher plus full pinned-copy corpus controls; no universal arbitrary-command claim'}
        self.expected = json.loads((self.repo / 'conformance/profiles/carddemo/inventory/carddemo-corpus.json').read_bytes())
        self.driver_hash = digest(Path(__file__).read_bytes())
        if self.driver_hash != args.driver_sha256:
            raise ValueError('driver differs from compiled owning input')
        supervisor = self.repo / 'tools/ci_assurance.py'
        self.dispatcher_hash = digest((self.repo / 'xtask/src/main.rs').read_bytes())
        if self.dispatcher_hash != args.dispatcher_sha256:
            raise ValueError('dispatcher differs from compiled owning input')
        self.supervisor_hash = digest(supervisor.read_bytes())
        if self.supervisor_hash != args.supervisor_sha256:
            raise ValueError('supervisor differs from compiled owning input')
        spec = importlib.util.spec_from_file_location('ci_assurance_corpus_owner', supervisor)
        self.supervisor = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self.supervisor)
        self.binaries = {}
        for label, binary in [('test', args.test_binary), ('xtask', args.xtask_binary)]:
            binary = binary.resolve(strict=True); metadata = binary.lstat()
            if not stat.S_ISREG(metadata.st_mode):
                raise ValueError('compiled producer is not a plain file')
            self.binaries[label] = (binary, metadata.st_dev, metadata.st_ino, digest(binary.read_bytes()))
        self.report['input_identities'] = {'driver_sha256': self.driver_hash,
            'supervisor_sha256': self.supervisor_hash,
            'inventory_sha256': digest((self.repo / 'conformance/profiles/carddemo/inventory/carddemo-corpus.json').read_bytes()),
            'dispatcher_sha256': self.dispatcher_hash,
            'compiled_binaries': {label: {'dev': row[1], 'inode': row[2], 'sha256': row[3]}
                                  for label, row in self.binaries.items()}}
        self.save()

    def verify(self):
        if identity(self.evidence) != self.evidence_id or identity(self.copy) != self.copy_id:
            raise ValueError('owned namespace identity changed')
        if not self.closed:
            raise ValueError('owned command closure is not established')

    def save(self):
        if identity(self.evidence) != self.evidence_id:
            raise ValueError('evidence namespace identity changed')
        payload = (json.dumps(self.report, indent=2) + '\n').encode()
        temporary = self.evidence / 'campaign.json.next'
        with temporary.open('wb') as stream:
            stream.write(payload); stream.flush(); os.fsync(stream.fileno())
        temporary.replace(self.evidence / 'campaign.json')

    def verify_inputs(self):
        for binary, dev, ino, expected in self.binaries.values():
            metadata = binary.lstat()
            if (metadata.st_dev, metadata.st_ino) != (dev, ino) or digest(binary.read_bytes()) != expected:
                raise ValueError('compiled producer identity changed')
        if digest(Path(__file__).read_bytes()) != self.driver_hash or digest((self.repo / 'tools/ci_assurance.py').read_bytes()) != self.supervisor_hash or digest((self.repo / 'xtask/src/main.rs').read_bytes()) != self.dispatcher_hash:
            raise ValueError('owning source changed')

    def run(self, label, command, *, env=None):
        self.verify()
        remaining = OUTER_SECONDS - (time.monotonic() - self.start)
        if remaining <= 0:
            raise ValueError('corpus campaign outer deadline exceeded')
        self.verify_inputs()
        ordinal = len(self.report['commands'])
        row = {'label': label, 'argv_sha256': digest(json.dumps(command).encode()),
               'owned_group_closed': False, 'status': 'running'}
        self.report['commands'].append(row)
        self.closed = False
        self.save()
        buffers = {'stdout': bytearray(), 'stderr': bytearray()}
        streams = {name: (self.evidence / f'{ordinal:02d}-{label}.{name}').open('wb') for name in buffers}
        def output(channel, chunk):
            streams[channel].write(chunk); streams[channel].flush()
            buffers[channel].extend(chunk)
        def closed(code):
            row['owned_group_closed'] = True
            row['closed_exit_code'] = code
            self.closed = True
        started = time.monotonic()
        try:
            code, error = self.supervisor._run_owned(command, self.repo, output,
                timeout_seconds=min(COMMAND_SECONDS, remaining),
                max_output_bytes=MAX_OUTPUT_BYTES - self.output_bytes,
                separate_stderr=True, env=env, on_closed=closed)
        finally:
            for stream in streams.values():
                stream.close()
        self.output_bytes += sum(map(len, buffers.values()))
        row.update({'exit_code': code, 'error': error, 'elapsed_seconds': time.monotonic() - started,
                    'stdout_bytes': len(buffers['stdout']), 'stderr_bytes': len(buffers['stderr'])})
        self.save()  # Retain the actual result even when closure is uncertain.
        self.verify()
        self.verify_inputs()
        row['status'] = 'closed'
        self.save()
        if error is not None:
            raise ValueError('owned command supervision failed')
        return code, bytes(buffers['stdout']), bytes(buffers['stderr'])

    def env(self, corpus=True):
        child = {key: value for key, value in os.environ.items() if not key.startswith('GIT_')}
        child.pop('CARDDEMO_CORPUS_DIR', None)
        if corpus:
            child['CARDDEMO_CORPUS_DIR'] = str(self.copy)
        return child

    def probe(self, case, inventory):
        child = self.env()
        if case == 'missing-directory':
            child['CARDDEMO_CORPUS_DIR'] = str(self.copy / 'not-present')
        child['CARDDEMO_CORPUS_CONTROL_CASE'] = case
        child['CARDDEMO_CORPUS_CONTROL_INVENTORY'] = str(inventory)
        command = [str(self.binaries['test'][0]), '--exact', PROBE, '--ignored', '--nocapture', '--test-threads=1']
        code, output, errors = self.run(case, command, env=child)
        if code != 0 or errors or b'running 1 test' not in output or b'test result: ok. 1 passed; 0 failed; 0 ignored;' not in output:
            raise ValueError('declared corpus refusal probe did not actually pass')
        records = [line[len(b'CD001_PROBE_JSON='):] for line in output.splitlines() if line.startswith(b'CD001_PROBE_JSON=')]
        if len(records) != 1:
            raise ValueError('missing or duplicate actual corpus probe result')
        result = json.loads(records[0])
        if result['case'] != case:
            raise ValueError('actual probe identity differs')
        if case == 'baseline':
            self.require_expected_receipt(result['receipt'])
            previous = self.report.get('tracked_paths_sha256')
            if previous is not None and previous != result['tracked_paths_sha256']:
                raise ValueError('baseline tracked path identities changed')
            self.report['tracked_paths_sha256'] = result['tracked_paths_sha256']
        elif case == 'same-count-corrupt' and result['tracked_paths_sha256'] != self.report.get('tracked_paths_sha256'):
            raise ValueError('same-count corruption changed tracked path identities')
        self.report['commands'][-1]['actual_probe'] = result
        self.save()

    def write(self, relative, raw):
        self.verify()
        path = self.copy / relative
        if path.is_symlink():
            raise ValueError('mutation source is a link')
        path.write_bytes(raw)

    def git(self, label, *arguments):
        code, _, _ = self.run(label, ['git', '-C', str(self.copy), *arguments], env=self.env())
        if code != 0:
            raise ValueError('owned Git control setup failed')

    def baseline(self, inventory):
        self.probe('baseline', inventory)

    def execute(self):
        self.verify()
        source_identity = identity(self.source)
        entries, total = scan(self.source)
        self.report['exclusive_source_copy'] = {'entries': len(entries), 'bytes': total,
            'manifest_sha256': digest(json.dumps(entries, sort_keys=True).encode()),
            'source_dev': source_identity[0], 'source_inode': source_identity[1]}
        copy_complete(self.source, self.copy, entries, self.verify)
        inventory = self.copy / '.git/corpus-control-inventory.json'
        original_inventory = (self.repo / 'conformance/profiles/carddemo/inventory/carddemo-corpus.json').read_bytes()
        self.write('.git/corpus-control-inventory.json', original_inventory)
        self.baseline(inventory)
        # Actual dispatcher routes, separate child environments, no global mutation.
        xtask = str(self.binaries['xtask'][0])
        code, output, errors = self.run('ordinary-abi-without-corpus', [xtask, 'abi-libraries', '--check'], env=self.env(False))
        if code != 0 or errors or output != b'abi-libraries: pass\n':
            raise ValueError('ordinary real CLI ABI gate requires unavailable corpus or failed')
        code, output, errors = self.run('carddemo-cli-without-corpus', [xtask, 'carddemo-corpus', '--check'], env=self.env(False))
        if code == 0 or output or b'carddemo.corpus.environment_missing:' not in errors:
            raise ValueError('real CardDemo CLI did not refuse missing corpus')
        if str(self.source).encode() in errors or str(self.copy).encode() in errors:
            raise ValueError('missing-environment refusal disclosed a local corpus path')
        code, output, errors = self.run('carddemo-cli-pinned-copy', [xtask, 'carddemo-corpus', '--check'], env=self.env())
        if code != 0 or errors or not output.endswith(b'\ncarddemo-corpus: pass\n'):
            raise ValueError('actual CLI pinned-corpus gate failed')
        actual = json.loads(output[:-len(b'carddemo-corpus: pass\n')])
        self.require_expected_receipt(actual)
        # Plain untracked dirt, physically missing tracked data, and same-count
        # corruption hidden from Git status all exercise the actual full verifier.
        self.write('corpus-control-untracked', b'dirty')
        self.probe('dirty', inventory)
        self.verify(); (self.copy / 'corpus-control-untracked').unlink()
        self.baseline(inventory)
        relative = 'app/cbl/CBTRN02C.cbl'; raw = (self.copy / relative).read_bytes()
        self.verify(); (self.copy / relative).unlink()
        self.probe('missing-tracked', inventory)
        self.write(relative, raw); self.baseline(inventory)
        self.git('mask-source-status', 'update-index', '--assume-unchanged', relative)
        changed = bytearray(raw); changed[-1] ^= 1
        self.write(relative, changed)
        self.probe('same-count-corrupt', inventory)
        self.write(relative, raw)
        self.git('restore-source-status', 'update-index', '--no-assume-unchanged', relative)
        self.baseline(inventory)
        # Every downstream identity has its own mutation; the original complete
        # inventory is restored and the full baseline rerun between controls.
        mutations = [
            ('commit-drift', lambda value: value.update(commit='0' * 40)),
            ('tree-drift', lambda value: value.update(tree='0' * 40)),
            ('license-sha-drift', lambda value: value.update(license_sha256='0' * 64)),
            ('unlicensed', lambda value: value.update(license='MIT')),
            ('content-drift', lambda value: value['executable_content_identity'].update(sha256='0' * 64)),
            ('runtime-archive-drift', lambda value: value['runtime_oracles'][0].update(sha256='0' * 64)),
            ('file-count-drift', lambda value: value['file_count_checks'][0].update(expected=value['file_count_checks'][0]['expected'] + 1)),
        ]
        for case, mutate in mutations:
            value = json.loads(original_inventory); mutate(value)
            self.write('.git/corpus-control-inventory.json', json.dumps(value).encode())
            self.probe(case, inventory)
            self.write('.git/corpus-control-inventory.json', original_inventory)
            self.baseline(inventory)
        self.git('local-origin', 'remote', 'set-url', 'origin', str(self.copy / 'private-origin'))
        self.probe('repository-local-path', inventory)
        self.git('restore-origin', 'remote', 'set-url', 'origin', self.expected['repository'])
        self.baseline(inventory)
        self.probe('missing-directory', inventory)
        # The unrelated source namespace must remain exactly as it was, including
        # actual Git metadata; this is an observed, non-atomic filesystem check.
        if identity(self.source) != source_identity or scan(self.source)[0] != entries:
            raise ValueError('original pinned source changed during owned controls')
        self.report['source_recheck'] = 'identical complete source manifest and directory identity'
        self.proven_requirements = list(REQUIREMENTS)
        self.report['status'] = 'comparisons-pass-cleanup-pending'
        self.save()

    def require_expected_receipt(self, actual):
        expected = self.expected
        if set(actual) != {'schema_version', 'status', 'repository', 'commit', 'tree', 'clean', 'license', 'content', 'runtime_oracles', 'file_counts'}:
            raise ValueError('actual corpus receipt fields differ')
        if actual['schema_version'] != 'mainframe-env.carddemo-corpus-receipt@1' or actual['status'] != 'pass' or actual['clean'] is not True:
            raise ValueError('actual corpus receipt is not the original pass contract')
        for name in ['repository', 'commit', 'tree']:
            if actual[name] != expected[name]:
                raise ValueError('actual pinned corpus identity differs')
        if actual['license'] != {'identifier': expected['license'], 'sha256': expected['license_sha256']}:
            raise ValueError('actual license identity differs')
        content = actual['content']
        if content['algorithm'] != expected['executable_content_identity']['algorithm'] or content['sha256'] != expected['executable_content_identity']['sha256']:
            raise ValueError('actual canonical content identity differs')
        if actual['runtime_oracles'] != [{'path': row['path'], 'sha256': row['sha256']} for row in expected['runtime_oracles']]:
            raise ValueError('actual runtime archive identities differ')
        if actual['file_counts'] != {row['id']: row['expected'] for row in expected['file_count_checks']}:
            raise ValueError('actual complete file-count identity set differs')
        raw = json.dumps(actual).encode()
        if str(self.copy).encode() in raw or str(self.source).encode() in raw:
            raise ValueError('actual public receipt disclosed a local corpus path')
        self.report['actual_pinned_receipt'] = actual

    def cleanup(self):
        self.verify()
        self.verify_inputs()
        if self.proven_requirements != REQUIREMENTS:
            raise ValueError('actual corpus campaign comparisons are incomplete')
        # Retain all actual comparisons/closure before removing any owned data.
        self.report['cleanup'] = {'status': 'owned-closure-and-identity-verified-removal-pending'}
        self.save()
        self.verify()
        shutil.rmtree(self.copy)
        if self.copy.exists() or self.copy.is_symlink():
            raise ValueError('owned copy remains after removal')
        self.report['cleanup'] = {'status': 'removed-and-absence-verified'}
        self.report['status'] = 'pass'
        self.report['requirements'] = list(self.proven_requirements)
        self.save()


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ['repo', 'corpus', 'evidence', 'test-binary', 'xtask-binary']:
        parser.add_argument('--' + name, required=True, type=Path)
    parser.add_argument('--driver-sha256', required=True)
    parser.add_argument('--supervisor-sha256', required=True)
    parser.add_argument('--dispatcher-sha256', required=True)
    args = parser.parse_args(argv)
    owner = None
    try:
        owner = Campaign.__new__(Campaign)
        owner.__init__(args)
        owner.execute()
        owner.cleanup()
    except BaseException as error:
        if owner is not None and hasattr(owner, 'report'):
            owner.report['status'] = 'fail-owned-copy-retained'
            owner.report['requirements'] = []
            owner.report['failure'] = type(error).__name__ + ': ' + str(error)
            owner.report['cleanup'] = {'status': 'retained; no success disposal after failed or unknown proof'}
            try:
                owner.save()
            except BaseException:
                pass
        raise
    print(json.dumps({'schema': owner.report['schema'], 'issue': 'CD-001',
                      'requirements': owner.report['requirements'], 'cleanup': owner.report['cleanup'],
                      'application_credit': 0, 'licensed_runtime_credit': 0}))
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
