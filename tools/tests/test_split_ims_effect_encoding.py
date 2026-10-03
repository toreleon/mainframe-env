"""The IMS execution owner remains subject to canonical replay checks."""
from pathlib import Path
import sys
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import check_effect_encoding as guard


class SplitImsEffectEncodingTests(unittest.TestCase):
    def test_execution_owner_cannot_drop_canonical_digest_or_migration(self):
        guard.check(guard.ROOT)
        original = Path.read_text
        execution_path = guard.ROOT / "crates/providers/mainframe-env-ims/src/service/execution.rs"
        for fragment in ("canonical_ims_request_digest(request)", "reconcile_legacy_replay"):
            with self.subTest(fragment=fragment):
                def altered(path, *args, **kwargs):
                    source = original(path, *args, **kwargs)
                    return source.replace(fragment, "removed_contract") if path == execution_path else source

                with patch.object(Path, "read_text", altered):
                    with self.assertRaisesRegex(ValueError, "IMS replay"):
                        guard.check(guard.ROOT)


if __name__ == "__main__":
    unittest.main()
