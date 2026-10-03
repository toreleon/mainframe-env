"""Provider row guards cover both the facade and its named persistence owner."""
from pathlib import Path
import sys
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import check_provider_rows as guard


class SplitProviderRowTests(unittest.TestCase):
    def test_required_and_forbidden_controls_remain_guarded_in_owner(self):
        guard.check(guard.ROOT)
        original = Path.read_text
        for family in ("ims", "mq"):
            child = "service_rows.rs" if family == "mq" else "service/rows.rs"
            owner = guard.ROOT / f"crates/providers/mainframe-env-{family}/src/{child}"
            for mutation in ("remove_atomic", "add_whole_state"):
                with self.subTest(family=family, mutation=mutation):
                    def altered(path, *args, **kwargs):
                        source = original(path, *args, **kwargs)
                        if path != owner:
                            return source
                        if mutation == "remove_atomic":
                            return source.replace(".mutate_provider_states_atomic(", ".removed_atomic(")
                        return source + "\nserde_json::to_vec(&state)\n"

                    with patch.object(Path, "read_text", altered):
                        with self.assertRaises(ValueError):
                            guard.check(guard.ROOT)


if __name__ == "__main__":
    unittest.main()
