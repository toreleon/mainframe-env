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
    def test_sortout_metadata_resolves_sortin_referback(self):
        jcl = """//PRC001.FILEOUT DD DISP=NEW,
//        DCB=(LRECL=350,RECFM=FB,BLKSIZE=0)
//SORTIN DD DISP=SHR,
//         DSN=AWS.M2.CARDDEMO.TRANSACT.BKUP(+1)
//SORTOUT DD DISP=NEW,
//         DCB=(*.SORTIN)
//TRANREPT DD DISP=NEW,
//         DCB=(LRECL=133,RECFM=FB,BLKSIZE=0)
"""
        source, selected, report = reference.tranrept_dataset_metadata(jcl)
        self.assertEqual(source, {"organization": "Sequential", "recfm": "FixedBlocked",
                                  "lrecl": 350, "ccsid": 37})
        self.assertEqual(selected, source)
        self.assertIsNot(selected, source)
        self.assertEqual(report["lrecl"], 133)
        with self.assertRaisesRegex(ValueError, "refer back"):
            reference.tranrept_dataset_metadata(jcl.replace("*.SORTIN", "*.OTHER"))

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
