from __future__ import annotations

import copy
import importlib.util
import json
from pathlib import Path
import sys
import unittest


TOOL = Path(__file__).resolve().parents[1] / "generate_spi1001_catalog.py"
SPEC = importlib.util.spec_from_file_location("generate_spi1001_catalog", TOOL)
catalog_tool = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = catalog_tool
SPEC.loader.exec_module(catalog_tool)


class Spi1001CatalogTests(unittest.TestCase):
    def setUp(self) -> None:
        self.authority, self.official = catalog_tool.source.validate_authority()

    def test_repository_catalog_is_fresh_and_exact(self) -> None:
        catalog_tool.check()
        catalog = json.loads((catalog_tool.ROOT / catalog_tool.OUTPUT_PATH).read_text())
        self.assertEqual(catalog["counts"], {
            "spi": 269,
            "fepi": 39,
            "total": 308,
            "raw_source_rows": 312,
            "deduplicated_source_rows": 4,
        })
        self.assertEqual(len(catalog["commands"]), 308)
        self.assertEqual(catalog["coverage_credit"], 0)

    def test_every_identity_is_semantically_blocked_and_unrouted(self) -> None:
        catalog = catalog_tool.build_catalog(self.authority, self.official)
        for command in catalog["commands"]:
            self.assertEqual(command["semantic_contract"]["state"], "blocked-source-gap")
            self.assertEqual(
                command["semantic_contract"]["blocked_facts"],
                catalog_tool.source.EXPECTED_BLOCKED_FACTS,
            )
            self.assertEqual(command["source_label_aliases"], [])
            self.assertEqual(
                command["runtime"],
                {"handler": None, "advertised": False, "automatically_registered": False},
            )
            self.assertEqual(command["coverage_credit"], 0)

    def test_spi_deduplication_and_fepi_shared_codes_are_preserved(self) -> None:
        commands = catalog_tool.build_catalog(self.authority, self.official)["commands"]
        by_label = {(row["interface"], row["label"]): row for row in commands}
        self.assertEqual(
            by_label[("SPI", "INQUIRE NETNAME")]["additional_eibfn_codes"], ["5206"]
        )
        allocate = by_label[("FEPI", "FEPI ALLOCATE PASSCONVID")]
        self.assertEqual(allocate["eibfn"], "8210")
        self.assertEqual(len(allocate["shared_eibfn_rows"]), 1)
        self.assertIn("fepi-commands:0003", allocate["shared_eibfn_rows"][0])

    def test_missing_official_identity_is_rejected(self) -> None:
        mutated = copy.deepcopy(self.official)
        unit = next(unit for unit in mutated["units"] if unit["id"] == "fepi-commands")
        unit["rows"].pop()
        with self.assertRaisesRegex(catalog_tool.CatalogError, "FEPI denominator"):
            catalog_tool.build_catalog(self.authority, mutated)

    def test_duplicate_official_identity_is_rejected(self) -> None:
        mutated = copy.deepcopy(self.official)
        unit = next(unit for unit in mutated["units"] if unit["id"] == "fepi-commands")
        unit["rows"][-1] = copy.deepcopy(unit["rows"][0])
        with self.assertRaisesRegex(catalog_tool.CatalogError, "row identities are duplicated"):
            catalog_tool.build_catalog(self.authority, mutated)

    def test_foreign_interface_locator_is_rejected(self) -> None:
        mutated = copy.deepcopy(self.official)
        unit = next(unit for unit in mutated["units"] if unit["id"] == "fepi-commands")
        unit["rows"][0]["source_locator"] = unit["rows"][0]["source_locator"].replace(
            "family:FEPI", "family:SPI"
        )
        with self.assertRaisesRegex(
            catalog_tool.source.SourceAuthorityError, "official interface differs"
        ):
            catalog_tool.build_catalog(self.authority, mutated)

    def test_source_gap_weakening_is_rejected(self) -> None:
        mutated = copy.deepcopy(self.authority)
        mutated["projection_boundary"]["semantic_authority"] = True
        with self.assertRaisesRegex(
            catalog_tool.source.SourceAuthorityError, "granted semantic"
        ):
            catalog_tool.build_catalog(mutated, self.official)

    def test_identity_digest_is_order_and_content_sensitive(self) -> None:
        commands = catalog_tool.build_catalog(self.authority, self.official)["commands"]
        reordered = list(reversed(commands))
        self.assertNotEqual(
            catalog_tool.identity_digest(commands), catalog_tool.identity_digest(reordered)
        )
        changed = copy.deepcopy(commands)
        changed[0]["label"] = "MUTATED"
        self.assertNotEqual(
            catalog_tool.identity_digest(commands), catalog_tool.identity_digest(changed)
        )

    def test_rust_registry_is_deterministic_and_has_no_dispatch_surface(self) -> None:
        catalog = catalog_tool.final_catalog()
        rendered = catalog_tool.render_rust_from_catalog(catalog).decode()
        self.assertEqual(rendered.count("CicsAdministrativeCommandIdentity {"), 308)
        self.assertIn("CICS_SPI_FEPI_AUTOMATIC_REGISTRATION: bool = false", rendered)
        self.assertIn("CICS_SPI_FEPI_PUBLIC_ROUTES: bool = false", rendered)
        self.assertNotIn("handler_id", rendered)
        self.assertNotIn("runtime_operation", rendered)

    def test_rust_registry_rejects_advertisement_mutation(self) -> None:
        catalog = catalog_tool.final_catalog()
        catalog["commands"][0]["runtime"]["advertised"] = True
        catalog["identity_sha256"] = catalog_tool.identity_digest(catalog["commands"])
        with self.assertRaisesRegex(catalog_tool.CatalogError, "executable or credited"):
            catalog_tool.render_rust_from_catalog(catalog)


if __name__ == "__main__":
    unittest.main()
