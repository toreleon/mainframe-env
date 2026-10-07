"""Offline transport regressions: synthetic bytes, real shared selection/import."""

from contextlib import redirect_stderr, redirect_stdout
from dataclasses import replace
import gzip
import hashlib
import io
import json
from pathlib import Path
import sys
import tarfile
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import ibm_docs
import ibm_docs_snapshot as snapshot


class SnapshotTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.cache = self.root / "source"
        self.cache.mkdir()
        self.output = self.root / "snapshot.tar.gz"
        self.destination = self.root / "destination"
        self.index = self.root / "index.json"
        self.a = ibm_docs.Scope("scope-a", "example", "baseline-a", "example", "unused-a")
        self.b = ibm_docs.Scope("scope-b", "example", "baseline-b", "example", "unused-b")
        self.bodies = [b"<h1>First synthetic topic</h1>", b"<h1>Second synthetic topic</h1>"]
        self.pins = [ibm_docs.Pin(f"PRODUCT/ref/{i}.html", "https://www.ibm.com/docs/example",
                                 self.digest(body), len(body), (scope,))
                     for i, (scope, body) in enumerate(zip([self.a, self.b], self.bodies))]
        self.toc_body = b'{"toc": "synthetic only"}'
        self.tocs = [ibm_docs.TocPin("https://www.ibm.com/docs/api/v1/toc/example?lang=en",
                                   self.digest(self.toc_body), (self.a, self.b))]
        for pin, body in zip(self.pins, self.bodies):
            (self.cache / pin.key).write_bytes(body)
        (self.cache / self.tocs[0].key).write_bytes(self.toc_body)
        loader = patch.object(ibm_docs, "load_pins", return_value=(self.pins, self.tocs))
        self.loader = loader.start()
        self.addCleanup(loader.stop)

    @staticmethod
    def digest(body):
        return hashlib.sha256(body).hexdigest()

    def pack(self):
        row = snapshot.pack("example-v1", ["scope-b", "scope-a", "scope-a"],
                            self.cache, self.output)
        self.write_index(row)
        return row

    def write_index(self, row, **changes):
        document = dict(schema_version=snapshot.SCHEMA,
                        repository="https://github.com/toreleon/mainframe-env-ibm-docs-cache.git",
                        revision="a" * 40, snapshots=[row])
        document.update(changes)
        self.index.write_text(json.dumps(document))

    def import_it(self):
        return snapshot.import_snapshot("example-v1", self.index, self.output, self.destination)

    def entries(self):
        return sorted([*( (pin.key, body, tarfile.REGTYPE)
                          for pin, body in zip(self.pins, self.bodies)),
                       (self.tocs[0].key, self.toc_body, tarfile.REGTYPE)])

    def malicious_archive(self, entries, *, tail=b"", truncate=0):
        # This hostile input writer is independent of the production packer.
        stream = io.BytesIO()
        with tarfile.open(fileobj=stream, mode="w", format=tarfile.GNU_FORMAT) as archive:
            for key, body, kind in entries:
                info = tarfile.TarInfo(key)
                info.mode = 0o444
                info.type = kind
                info.size = len(body) if kind == tarfile.REGTYPE else 0
                if kind in {tarfile.SYMTYPE, tarfile.LNKTYPE}:
                    info.linkname = self.pins[0].key
                archive.addfile(info, io.BytesIO(body) if kind == tarfile.REGTYPE else None)
        data = gzip.compress(stream.getvalue() + tail, mtime=0)
        if truncate:
            data = data[:-truncate]
        self.output.write_bytes(data)
        row = dict(id="example-v1", archive="snapshots/example-v1.tar.gz",
                   sha256=self.digest(data), bytes=len(data), scopes=["scope-a", "scope-b"],
                   pins_sha256=snapshot.fingerprint(snapshot.selected(["scope-a", "scope-b"])[2]),
                   topics=2, tocs=1, semantic_authority=False, coverage_credit=0)
        self.write_index(row)

    def rejected_without_import(self):
        with patch.object(ibm_docs, "import_cache", wraps=ibm_docs.import_cache) as shared:
            with self.assertRaises((ValueError, OSError, EOFError, tarfile.TarError)):
                self.import_it()
            shared.assert_not_called()
        self.assertFalse(self.destination.exists())

    def test_deterministic_complete_roundtrip_and_idempotent_shared_import(self):
        row = self.pack()
        first = self.output.read_bytes()
        second_path = self.root / "second.tar.gz"
        second = snapshot.pack("example-v1", ["scope-a", "scope-b"], self.cache, second_path)
        self.assertEqual(first, second_path.read_bytes())
        self.assertEqual(row, second)
        self.assertEqual(first[4:8], bytes(4))  # gzip mtime0
        self.assertEqual(first[3] & 8, 0)      # gzip has no original filename
        with tarfile.open(fileobj=io.BytesIO(gzip.decompress(first))) as archive:
            members = archive.getmembers()
            self.assertEqual([m.name for m in members], sorted(key for key, _, _ in self.entries()))
            self.assertTrue(all(m.isreg() and m.uid == m.gid == m.mtime == 0
                                and m.mode == 0o444 and not m.uname and not m.gname
                                for m in members))
        with patch.object(ibm_docs, "import_cache", wraps=ibm_docs.import_cache) as shared:
            result = self.import_it()
            self.assertEqual(result["imported"], 3)
            shared.assert_called_once()
        self.assertFalse(result["semantic_authority"])
        self.assertEqual(result["coverage_credit"], 0)
        for pin, body in zip(self.pins, self.bodies):
            self.assertEqual(ibm_docs.cached_bytes(self.destination, ibm_docs.pin_target(pin)), body)
        self.assertEqual(ibm_docs.cached_bytes(self.destination, ibm_docs.toc_target(self.tocs[0])),
                         self.toc_body)
        self.assertEqual(self.import_it()["already_present"], 3)
        with self.assertRaises(ValueError):
            snapshot.pack("example-v1", ["scope-a"], self.cache, self.output)
        self.assertEqual(self.output.read_bytes(), first)

    def test_fingerprint_exact_canonical_json_and_shared_null_toc_size(self):
        row = self.pack()
        raw = [{"key": pin.key, "sha256": pin.sha256, "size": pin.size} for pin in self.pins]
        raw.append({"key": self.tocs[0].key, "sha256": self.tocs[0].sha256, "size": None})
        encoded = json.dumps(sorted(raw, key=lambda r: r["key"]), sort_keys=True,
                             separators=(",", ":"), ensure_ascii=True).encode("utf-8")
        self.assertEqual(row["pins_sha256"], self.digest(encoded))
        self.assertEqual(row["topics"], 2)
        self.assertEqual(row["tocs"], 1)
        self.assertEqual(row["scopes"], ["scope-a", "scope-b"])

    def test_pack_verifies_missing_mismatch_and_symlink_before_publication(self):
        path = self.cache / self.pins[0].key
        for mutant in ["missing", "mismatch", "symlink"]:
            with self.subTest(mutant=mutant):
                if path.exists() or path.is_symlink():
                    path.unlink()
                if mutant == "mismatch":
                    path.write_bytes(b"x" * len(self.bodies[0]))
                elif mutant == "symlink":
                    path.symlink_to(self.cache / self.pins[1].key)
                with self.assertRaises((ValueError, FileNotFoundError)):
                    self.pack()
                self.assertFalse(self.output.exists())
                self.assertFalse(list(self.root.glob(".snapshot-*")))
        path.unlink()
        (self.cache / self.pins[0].legacy_key).write_bytes(self.bodies[0])
        self.pack()  # shared verified legacy lookup is retained

    def test_pack_missing_or_bad_toc_and_aggregate_limit(self):
        path = self.cache / self.tocs[0].key
        path.unlink()
        with self.assertRaises(FileNotFoundError):
            self.pack()
        path.write_bytes(b"bad toc")
        with self.assertRaises(ValueError):
            self.pack()
        path.write_bytes(self.toc_body)
        with patch.object(ibm_docs, "MAX_IMPORT", sum(map(len, self.bodies))):
            with self.assertRaises(ValueError):
                self.pack()
        self.assertFalse(self.output.exists())

    def test_archive_digest_and_size_checked_before_decompression(self):
        row = self.pack()
        original = self.output.read_bytes()
        for data in [original[:-1], bytes([original[0] ^ 1]) + original[1:], original + b"x"]:
            with self.subTest(size=len(data)):
                self.output.write_bytes(data)
                with patch.object(gzip, "GzipFile", side_effect=AssertionError("must not decompress")):
                    self.rejected_without_import()
        self.assertEqual(row["bytes"], len(original))

    def test_truncated_corrupt_gzip_even_with_matching_locator_digest(self):
        for mutant in ["truncated", "crc", "not-gzip"]:
            with self.subTest(mutant=mutant):
                self.malicious_archive(self.entries(), truncate=5 if mutant == "truncated" else 0)
                data = self.output.read_bytes()
                if mutant == "crc":
                    data = data[:-8] + bytes([data[-8] ^ 1]) + data[-7:]
                elif mutant == "not-gzip":
                    data = b"not a gzip archive"
                self.output.write_bytes(data)
                doc = json.loads(self.index.read_text())
                doc["snapshots"][0].update(sha256=self.digest(data), bytes=len(data))
                self.index.write_text(json.dumps(doc))
                self.rejected_without_import()

    def test_incomplete_and_mismatched_pinned_archive_no_partial_cache(self):
        entries = self.entries()
        for mutant in [entries[:-1], [(entries[0][0], b"x" * len(entries[0][1]), tarfile.REGTYPE),
                                      *entries[1:]]]:
            with self.subTest(entries=len(mutant)):
                self.malicious_archive(mutant)
                self.rejected_without_import()

    def test_unsafe_duplicate_unregistered_and_nonregular_members(self):
        entries = self.entries()
        mutants = [entries + [entries[0]], entries + [("unknown", b"x", tarfile.REGTYPE)],
                   entries + [("../escape", b"x", tarfile.REGTYPE)],
                   entries + [("/absolute", b"x", tarfile.REGTYPE)],
                   entries + [("x\\y", b"x", tarfile.REGTYPE)]]
        mutants.extend([(entries[0][0], b"", kind), *entries[1:]] for kind in
                       [tarfile.SYMTYPE, tarfile.LNKTYPE, tarfile.DIRTYPE, tarfile.CHRTYPE,
                        tarfile.FIFOTYPE])
        for mutant in mutants:
            with self.subTest(last=mutant[-1], first=mutant[0]):
                self.malicious_archive(mutant)
                self.rejected_without_import()

    def test_truncated_tar_and_nonzero_trailing_data(self):
        for tail in [b"hidden unregistered bytes", bytes(511) + b"x"]:
            self.malicious_archive(self.entries(), tail=tail)
            self.rejected_without_import()
        self.malicious_archive(self.entries())
        decoded = gzip.decompress(self.output.read_bytes())[:700]
        data = gzip.compress(decoded, mtime=0)
        self.output.write_bytes(data)
        doc = json.loads(self.index.read_text())
        doc["snapshots"][0].update(sha256=self.digest(data), bytes=len(data))
        self.index.write_text(json.dumps(doc))
        self.rejected_without_import()

    def test_compressed_decompressed_entry_and_total_byte_limits(self):
        self.malicious_archive(self.entries())
        for owner, attribute, bound in [(snapshot, "MAX_ARCHIVE", 10),
                                         (snapshot, "MAX_TAR", 1024),
                                         (ibm_docs, "MAX_FILE", 1),
                                         (ibm_docs, "MAX_IMPORT", 1),
                                         (ibm_docs, "MAX_ARCHIVE_ENTRIES", 2)]:
            with self.subTest(limit=attribute), patch.object(owner, attribute, bound):
                self.rejected_without_import()

    def test_archive_actual_extra_entry_and_toc_aggregate_are_bounded(self):
        self.malicious_archive(self.entries() + [("extra", b"x", tarfile.REGTYPE)])
        with patch.object(ibm_docs, "MAX_ARCHIVE_ENTRIES", 3):
            self.rejected_without_import()
        self.malicious_archive(self.entries())
        # Declared topic sizes fit; undeclared TOC bytes push the actual archive over the cap.
        with patch.object(ibm_docs, "MAX_IMPORT", sum(map(len, self.bodies))):
            self.rejected_without_import()

    def test_atomic_publication_race_never_replaces_competing_archive(self):
        real_link = snapshot.os.link
        def competing_link(source, destination, **kwargs):
            Path(destination).write_bytes(b"competing archive remains")
            return real_link(source, destination, **kwargs)
        with patch.object(snapshot.os, "link", side_effect=competing_link):
            with self.assertRaises(FileExistsError):
                self.pack()
        self.assertEqual(self.output.read_bytes(), b"competing archive remains")
        self.assertFalse(list(self.root.glob(".snapshot-*")))

    def test_shared_pinned_authority_changes_fail_before_decompression(self):
        self.pack()
        for changed in [replace(self.pins[0], sha256="0" * 64),
                        replace(self.pins[0], size=self.pins[0].size + 1)]:
            with self.subTest(changed=changed), patch.object(
                    ibm_docs, "load_pins", return_value=([changed, self.pins[1]], self.tocs)):
                with patch.object(gzip, "GzipFile", side_effect=AssertionError("too early")):
                    self.rejected_without_import()
        with patch.object(ibm_docs, "load_pins", return_value=(self.pins, [replace(
                self.tocs[0], sha256="f" * 64)])):
            self.rejected_without_import()

    def test_preexisting_conflict_and_link_preserved_no_new_cache_entries(self):
        self.pack()
        self.destination.mkdir()
        path = self.destination / self.pins[1].key
        path.write_bytes(b"preexisting conflict")
        for symlink in [False, True]:
            with self.subTest(symlink=symlink):
                if symlink:
                    path.unlink()
                    path.symlink_to(self.cache / self.pins[1].key)
                before = sorted(p.name for p in self.destination.iterdir())
                with patch.object(ibm_docs, "import_cache", wraps=ibm_docs.import_cache) as shared:
                    with self.assertRaises(ValueError):
                        self.import_it()
                    shared.assert_not_called()
                self.assertEqual(sorted(p.name for p in self.destination.iterdir()), before)
                self.assertTrue(path.is_symlink() if symlink else path.read_bytes() == b"preexisting conflict")

    def test_locator_schema_fields_revision_counts_and_zero_credit(self):
        row = self.pack()
        top_mutants = [("schema_version", "wrong"), ("revision", "main"),
                       ("revision", "a" * 39), ("repository", "http://github.com/a/b.git"),
                       ("repository", "https://secret@github.com/a/b.git"),
                       ("repository", "https://github.com/a/b.git?token=secret"),
                       ("repository", "https://example.com/a/b.git")]
        for field, value in top_mutants:
            with self.subTest(field=field, value=value):
                self.write_index(row, **{field: value})
                self.rejected_without_import()
        for field, value in [("topics", 3), ("tocs", 2), ("bytes", True),
                             ("sha256", "g" * 64), ("pins_sha256", "0" * 64),
                             ("scopes", ["scope-b", "scope-a"]),
                             ("scopes", ["scope-a", "scope-a"]),
                             ("scopes", ["not-registered"]), ("archive", "../snapshot.tar.gz"),
                             ("id", "../bad"), ("semantic_authority", True),
                             ("coverage_credit", 1), ("coverage_credit", False)]:
            with self.subTest(field=field):
                mutant = dict(row, **{field: value})
                self.write_index(mutant)
                self.rejected_without_import()
        self.write_index(row, snapshots=[row, row])
        self.rejected_without_import()
        self.write_index(row, repository="https://github.com/another-owner/other-cache.git")
        self.assertEqual(snapshot.snapshot_row(self.index, row["id"]), row)

    def test_json_duplicate_fields_and_oversized_index(self):
        row = self.pack()
        text = self.index.read_text()
        self.index.write_text(text.replace('"revision":', '"revision":"b", "revision":'))
        self.rejected_without_import()
        self.write_index(row)
        with patch.object(snapshot, "MAX_INDEX", 20):
            self.rejected_without_import()

    def test_all_actual_same_repository_worktree_roots_are_protected(self):
        roots = snapshot.repository_roots()
        self.assertIn(ibm_docs.docs_api.REPOSITORY.resolve(), roots)
        for root in roots:
            with self.subTest(root=root):
                for path in [root / "topic-cache", root / "snapshot.tar.gz"]:
                    with self.assertRaises(ValueError):
                        snapshot.external_path(path, roots)
        # A separate private cache clone (not in the same Git worktree inventory) is allowed.
        private = self.root / "private-cache-clone"
        private.mkdir()
        (private / ".git").mkdir()
        self.assertEqual(snapshot.external_path(private / "snapshots", roots),
                         private / "snapshots")

    def test_paths_root_home_relative_symlink_and_repository_alias(self):
        roots = snapshot.repository_roots()
        for path in [Path("relative"), Path("/"), Path.home()]:
            with self.subTest(path=path), self.assertRaises(ValueError):
                snapshot.external_path(path, roots)
        for target in [self.root / "outside", ibm_docs.docs_api.REPOSITORY]:
            alias = self.root / "alias"
            alias.symlink_to(target, target_is_directory=True)
            with self.assertRaises(ValueError):
                snapshot.external_path(alias / "cache", roots)
            alias.unlink()
        self.output.symlink_to(self.root / "missing")
        with self.assertRaises(ValueError):
            self.pack()
        self.assertTrue(self.output.is_symlink())

    def test_cli_pack_import_default_index_and_nonzero_error_no_token_echo(self):
        with redirect_stdout(io.StringIO()) as out:
            self.assertEqual(snapshot.main(["pack", "--id", "example-v1", "--scope", "scope-a",
                                            "--scope", "scope-b", "--cache", str(self.cache),
                                            "--archive", str(self.output)]), 0)
        row = json.loads(out.getvalue())
        self.write_index(row)
        with patch.object(snapshot, "DEFAULT_INDEX", self.index), redirect_stdout(io.StringIO()):
            self.assertEqual(snapshot.main(["import", "--snapshot", "example-v1", "--cache",
                                            str(self.destination), "--archive", str(self.output)]), 0)
        self.write_index(row, repository="https://private-token@github.com/a/b.git")
        with redirect_stderr(io.StringIO()) as err:
            self.assertEqual(snapshot.main(["import", "--snapshot", "example-v1", "--index",
                                            str(self.index), "--cache", str(self.destination),
                                            "--archive", str(self.output)]), 1)
        self.assertNotIn("private-token", err.getvalue())

    def test_actual_main_shared_selection_rejects_unregistered_scope(self):
        with patch.object(ibm_docs, "load_pins", self.real_load_pins):
            with redirect_stderr(io.StringIO()):
                self.assertEqual(snapshot.main(["pack", "--id", "real-selection", "--scope",
                                                "unregistered-snapshot-fixture", "--cache",
                                                str(self.cache), "--archive", str(self.output)]), 1)
        self.assertFalse(self.output.exists())

    # Assigned before fixture patches: the actual committed manifest loader.
    real_load_pins = staticmethod(ibm_docs.load_pins)


if __name__ == "__main__":
    unittest.main()
