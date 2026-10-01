import importlib.util
from pathlib import Path
import tempfile
import unittest


TOOL = Path(__file__).resolve().parents[1] / "merge_documentation_manifest.py"
REPOSITORY = TOOL.parents[1]
SPEC = importlib.util.spec_from_file_location("merge_documentation_manifest", TOOL)
assert SPEC and SPEC.loader
MERGER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MERGER)


def row(path: str, commands: int = 0) -> dict:
    return {
        "normative": False,
        "path": path,
        "relative_links": 0,
        "sha256": f"sha256:{path}",
        "title": path,
        "xtask_commands": commands,
    }


def manifest(documents: list[dict]) -> dict:
    return {
        "counts": {
            "markdown_documents": len(documents),
            "navigation_groups": 1,
            "normative_documents": 0,
            "xtask_commands": sum(item["xtask_commands"] for item in documents),
        },
        "documents": documents,
        "navigation": [{"entries": [], "heading": "Docs"}],
        "schema_version": "mainframe-env.documentation-manifest@1",
    }


class DocumentationManifestMergeTests(unittest.TestCase):
    def test_writer_matches_the_checked_in_generator_format(self):
        source = REPOSITORY / "docs/generated/documentation-manifest.json"
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "manifest.json"
            MERGER.write_atomic(output, MERGER.load(source))
            self.assertEqual(output.read_bytes(), source.read_bytes())

    def test_independent_documents_merge_and_counts_are_recomputed(self):
        base = manifest([row("README.md", 1)])
        ours = manifest([row("README.md", 1), row("docs/ours.md", 2)])
        theirs = manifest([row("README.md", 1), row("docs/theirs.md", 3)])
        merged = MERGER.merge_manifest(base, ours, theirs)
        self.assertEqual(
            [item["path"] for item in merged["documents"]],
            ["README.md", "docs/ours.md", "docs/theirs.md"],
        )
        self.assertEqual(merged["counts"]["markdown_documents"], 3)
        self.assertEqual(merged["counts"]["xtask_commands"], 6)

    def test_same_document_divergence_fails_closed(self):
        base = manifest([row("README.md")])
        ours = manifest([row("README.md")])
        theirs = manifest([row("README.md")])
        ours["documents"][0]["sha256"] = "sha256:ours"
        theirs["documents"][0]["sha256"] = "sha256:theirs"
        with self.assertRaises(MERGER.MergeConflict):
            MERGER.merge_manifest(base, ours, theirs)

    def test_one_sided_document_change_is_preserved(self):
        base = manifest([row("README.md")])
        ours = manifest([row("README.md")])
        theirs = manifest([row("README.md")])
        theirs["documents"][0]["sha256"] = "sha256:updated"
        merged = MERGER.merge_manifest(base, ours, theirs)
        self.assertEqual(merged["documents"][0]["sha256"], "sha256:updated")


if __name__ == "__main__":
    unittest.main()
