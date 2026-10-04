"""The sole original-dispatch owner and parent lifecycle controls are mandatory."""
from pathlib import Path
import shutil
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import check_effect_encoding as guard


PARENT = Path("crates/kernel/mainframe-env-interpreter/src/coordinator.rs")
CHILD = PARENT.parent / "coordinator/original_dispatch.rs"
FILES = (
    PARENT,
    CHILD,
    Path("crates/contracts/mainframe-env-host-api/src/service.rs"),
    Path("crates/contracts/mainframe-env-host-api/src/canonical.rs"),
    Path("crates/contracts/mainframe-env-execution-api/src/lifecycle_notification.rs"),
    Path("crates/providers/mainframe-env-mq/src/service.rs"),
    Path("crates/providers/mainframe-env-mq/src/message.rs"),
    Path("crates/providers/mainframe-env-ims/src/service.rs"),
    Path("crates/providers/mainframe-env-ims/src/service/execution.rs"),
    Path("crates/providers/mainframe-env-db2/src/service.rs"),
    Path("crates/providers/mainframe-env-racf/src/command_processor.rs"),
    Path("crates/providers/mainframe-env-racf/src/saf.rs"),
    Path("crates/providers/mainframe-env-racf/src/database.rs"),
    Path("crates/stores/mainframe-env-store/src/durable.rs"),
)


class SplitCoordinatorEffectEncodingTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="coordinator-effect-guard-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        for relative in FILES:
            target = self.root / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(guard.ROOT / relative, target)

    def remove(self, path, fragment):
        target = self.root / path
        source = target.read_text()
        self.assertEqual(source.count(fragment), 1)
        target.write_text(source.replace(fragment, "removed_control"))

    def test_actual_split_passes(self):
        guard.check(self.root)

    def test_child_request_digest_is_mandatory(self):
        self.remove(CHILD, "canonical_request_digest(&effect.request)")
        with self.assertRaisesRegex(ValueError, "original dispatch"):
            guard.check(self.root)

    def test_child_result_digest_is_mandatory(self):
        self.remove(CHILD, "let result_digest = match canonical_result_digest(&result.outcome)")
        with self.assertRaisesRegex(ValueError, "original dispatch"):
            guard.check(self.root)

    def test_child_replay_result_digest_is_mandatory(self):
        self.remove(CHILD, "let replay_digest = match canonical_result_digest(&result.outcome)")
        with self.assertRaisesRegex(ValueError, "original dispatch"):
            guard.check(self.root)

    def test_parent_payload_is_mandatory(self):
        self.remove(PARENT, "payload: lifecycle_payload(&event.kind)")
        with self.assertRaisesRegex(ValueError, "coordinator"):
            guard.check(self.root)

    def test_parent_notification_is_mandatory(self):
        self.remove(PARENT, "mainframe_env_execution_api::lifecycle_notification_payload(kind)")
        with self.assertRaisesRegex(ValueError, "coordinator"):
            guard.check(self.root)

    def test_parent_topic_is_mandatory(self):
        self.remove(PARENT, 'LIFECYCLE_OUTBOX_TOPIC: &str = "execution.lifecycle.v1"')
        with self.assertRaisesRegex(ValueError, "lifecycle topic"):
            guard.check(self.root)

    def test_parent_module_is_mandatory(self):
        self.remove(PARENT, "mod original_dispatch;")
        with self.assertRaisesRegex(ValueError, "coordinator"):
            guard.check(self.root)

    def test_parent_construction_is_mandatory(self):
        self.remove(PARENT, "let dispatch = original_dispatch::OriginalDispatch {")
        with self.assertRaisesRegex(ValueError, "coordinator"):
            guard.check(self.root)

    def test_parent_callsite_is_mandatory(self):
        self.remove(PARENT, "match dispatch.dispatch(effect) {")
        with self.assertRaisesRegex(ValueError, "coordinator"):
            guard.check(self.root)

    def test_missing_child_has_no_fallback(self):
        (self.root / CHILD).unlink()
        with self.assertRaises(FileNotFoundError):
            guard.check(self.root)

    def test_child_persisted_debug_is_rejected(self):
        target = self.root / CHILD
        target.write_text('let digest = Sha256::digest(format!("{request:?}").as_bytes());\n' + target.read_text())
        with self.assertRaisesRegex(ValueError, "diagnostic formatting"):
            guard.check(self.root)

    def test_comments_and_string_decoys_cannot_replace_controls(self):
        for path, fragment in (
            (PARENT, "mod original_dispatch;"),
            (PARENT, "match dispatch.dispatch(effect) {"),
            (PARENT, "payload: lifecycle_payload(&event.kind)"),
            (CHILD, "canonical_request_digest(&effect.request)"),
            (CHILD, "let result_digest = match canonical_result_digest(&result.outcome)"),
            (CHILD, "let replay_digest = match canonical_result_digest(&result.outcome)"),
        ):
            for decoy in ("// {}\n", "/* outer /* nested */ {} */", 'let decoy = "{}";'):
                with self.subTest(path=path, fragment=fragment, decoy=decoy):
                    target = self.root / path
                    original = target.read_text()
                    self.assertEqual(original.count(fragment), 1)
                    altered = original.replace(fragment, "removed_control")
                    marker = altered.index("#[cfg(test)]")
                    target.write_text(altered[:marker] + decoy.format(fragment) + "\n" + altered[marker:])
                    try:
                        with self.assertRaisesRegex(ValueError, "coordinator"):
                            guard.check(self.root)
                    finally:
                        target.write_text(original)


if __name__ == "__main__":
    unittest.main()
