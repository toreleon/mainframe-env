"""Framing regression for the CardDemo TRANREPT reference."""

import importlib.util
from pathlib import Path
import unittest


SCRIPT = (
    Path(__file__).resolve().parents[2]
    / "conformance/tools/carddemo_tranrept_reference.py"
)
SPEC = importlib.util.spec_from_file_location("carddemo_tranrept_reference", SCRIPT)
reference = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(reference)


class FramingTest(unittest.TestCase):
    def test_two_one_byte_records_with_sequential_metadata(self):
        self.assertEqual(
            reference.framed_dataset([b"A", b"B"], 1, 2),
            "826b560e4d5af5bf1f2d6714470eaab41d3354d23f3c2c5d403afcd173d228ad",
        )

    def test_rejects_non_fixed_record(self):
        with self.assertRaises(ValueError):
            reference.framed_dataset([b"AB"], 1, 2)

    def test_variable_dataset_metadata_with_fixed_logical_records(self):
        self.assertEqual(
            reference.framed_dataset(
                [b"A", b"B"], 32760, 3, record_format="Variable", record_length=1
            ),
            "4ea320ef0c1c65596560a659d600a4f21cd42cc0071d3649b9b330035fbceef6",
        )


if __name__ == "__main__":
    unittest.main()
