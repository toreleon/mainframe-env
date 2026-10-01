"""Global CICS plan tag injectivity and 64-bit reserved-range regression."""

import re
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
TAGS = (ROOT / "crates/foundation/mainframe-env-ir/src/cics_plan/codec_tags.rs").read_text()
OUTPUT_TAGS = (
    ROOT / "crates/foundation/mainframe-env-ir/src/cics_plan/codec_tags/output.rs"
).read_text()
OPTION_TAGS = (
    ROOT / "crates/foundation/mainframe-env-ir/src/cics_plan/codec_tags/options.rs"
).read_text()
ASSIGN = (ROOT / "crates/foundation/mainframe-env-ir/src/cics_plan/assign.rs").read_text()


def body(name):
    source = (
        OUTPUT_TAGS
        if name.startswith("output")
        else OPTION_TAGS
        if name.startswith("option")
        else TAGS
    )
    start = source.index(f"fn {name}(")
    return source[start : source.index("\n}", start)]


def mappings(kind, rust_type):
    forward = [
        (name, int(tag))
        for name, tag in re.findall(
            rf"{rust_type}::([A-Za-z0-9_]+) => ([0-9]+),", body(f"{kind}_tag")
        )
    ]
    reverse = [
        (name, int(tag))
        for tag, name in re.findall(
            rf"([0-9]+) => Ok\({rust_type}::([A-Za-z0-9_]+)\)",
            body(f"{kind}_from_tag"),
        )
    ]
    return forward, reverse


class CicsStorage64Tags(unittest.TestCase):
    def test_every_static_tag_is_globally_unique_and_round_trips(self):
        expected = {
            "operation": ("CicsPlanOperation", {"Getmain64": 72, "Freemain64": 73}),
            "operand": (
                "CicsOperandName",
                {
                    "Flength64": 172,
                    "Location64": 173,
                    "Abi64": 174,
                    "DataPointer64": 175,
                    "DataArea64": 176,
                },
            ),
            "option": (
                "CicsPlanOption",
                {
                    "CicsDataKey64": 108,
                    "UserDataKey64": 109,
                    "Shared64": 110,
                    "Executable64": 111,
                },
            ),
            "output": ("CicsOutputName", {"SetPointer64": 232}),
        }
        reserved = {
            "operation": range(72, 74),
            "operand": range(172, 182),
            "option": range(108, 116),
            "output": range(232, 240),
        }
        for kind, (rust_type, additions) in expected.items():
            with self.subTest(kind=kind):
                forward, reverse = mappings(kind, rust_type)
                self.assertEqual(len(forward), len(set(forward)))
                self.assertEqual(len(forward), len({name for name, _ in forward}))
                self.assertEqual(len(forward), len({tag for _, tag in forward}))
                self.assertEqual(set(forward), set(reverse))
                self.assertEqual(
                    {name: tag for name, tag in forward if tag in reserved[kind]},
                    additions,
                )

        names = ASSIGN.split("pub const CICS_ASSIGN_OUTPUT_NAMES: &[&str] = &[", 1)[1].split("];", 1)[0]
        assign_count = len(re.findall(r'"[A-Z0-9]+"', names))
        assign_tags = {
            13 + tag if tag < 78 else 96 + tag - 78 for tag in range(assign_count)
        }
        static_output_tags = {tag for _, tag in mappings("output", "CicsOutputName")[0]}
        self.assertFalse(assign_tags & static_output_tags)
        self.assertFalse(assign_tags & set(reserved["output"]))


if __name__ == "__main__":
    unittest.main()
