"""Offline tests for the probe-record figure checker.

`report_probe_figures.py --check` exists because a checker nobody runs is a
checker that does not exist. These tests are what make it run: they assert the
same property its exit status does, so a number in
`docs/research/publication-source-probe.md` that disagrees with the artifact it
was read from turns the Python suite red rather than waiting for a reviewer to
notice on the sixth wave.

The load-bearing test in this file is
`StaleRecordTests.test_one_changed_digit_in_the_record_fails_the_check`. A
checker that passes because it is checking nothing is worse than no checker, so
this file does not merely assert that the record is currently clean -- it builds
a tree whose artifacts are this repository's and whose record has one digit
moved, and asserts that `--check` finds it and exits non-zero. If that test ever
starts passing vacuously, the mutation tests below stop failing, and that is
itself the alarm.

Nothing here touches the network and nothing here reads a publication body.
Every figure is read from a committed artifact, and the mutated trees are built
in `$TMPDIR` out of symlinks, so no test writes inside the repository.
"""

from __future__ import annotations

import contextlib
import io
import json
import re
import sys
import tempfile
import unittest
from pathlib import Path

TOOLS = Path(__file__).resolve().parents[1]
REPOSITORY = TOOLS.parents[1]

sys.path.insert(0, str(TOOLS))

import report_probe_figures as FIGURES  # noqa: E402

#: Top-level directories the tool reads an artifact out of. A mutated tree
#: symlinks these and supplies its own `docs/`, so the artifacts under test are
#: the committed ones and only the prose differs.
ARTIFACT_TREES = ("conformance", "crates", "xtask")


def record_text() -> str:
    return (REPOSITORY / FIGURES.RECORD).read_text(encoding="utf-8")


def restate(text: str, key: str, value: str) -> str:
    """The record with the first citation of `key` quoting `value` instead.

    Rewriting through the tool's own citation pattern rather than through a
    literal search is deliberate: a mutation that the pattern cannot find is a
    mutation `--check` could never have caught, and would make a passing test
    out of a blind spot.
    """
    for match in FIGURES.CITATION.finditer(text):
        if match["key"] == key:
            return text[: match.start("value")] + value + text[match.end("value") :]
    raise AssertionError(f"{key} is cited nowhere in {FIGURES.RECORD}")


def unmark(text: str, key: str) -> str:
    """The record with the first citation of `key` reduced to a bare number."""
    for match in FIGURES.CITATION.finditer(text):
        if match["key"] == key:
            return text[: match.start()] + match["value"] + text[match.end() :]
    raise AssertionError(f"{key} is cited nowhere in {FIGURES.RECORD}")


class TreeCase(unittest.TestCase):
    """A case that can build a tree with this repository's artifacts and its own record."""

    def tree(self, text: str) -> Path:
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        root = Path(temporary.name)
        for name in ARTIFACT_TREES:
            (root / name).symlink_to(REPOSITORY / name, target_is_directory=True)
        record = root / FIGURES.RECORD
        record.parent.mkdir(parents=True)
        record.write_text(text, encoding="utf-8")
        return root

    def check(self, text: str):
        return FIGURES.check(self.tree(text))

    def exit_status(self, text: str) -> tuple[int, str]:
        root = self.tree(text)
        captured = io.StringIO()
        with contextlib.redirect_stdout(captured):
            status = FIGURES.main(["--check", "--root", str(root)])
        return status, captured.getvalue()


class MarkerGrammarTests(unittest.TestCase):
    """What counts as a citation, and what deliberately does not."""

    def cited(self, text: str) -> list[tuple[str, int]]:
        return [
            (match["key"], FIGURES.quoted(match["value"]))
            for match in FIGURES.CITATION.finditer(text)
        ]

    def test_a_marker_is_an_html_comment_and_renders_as_nothing(self):
        # The whole point of the grammar: a reader sees `865`, and the checker
        # sees which figure `865` claims to be.
        marked = "865<!--f:catalog.topic_located_total-->"
        self.assertEqual(re.sub(r"<!--.*?-->", "", marked), "865")
        self.assertEqual(self.cited(marked), [("catalog.topic_located_total", 865)])

    def test_thousands_separators_are_part_of_the_number(self):
        self.assertEqual(self.cited("1,506<!--f:catalog.rows_total-->"), [("catalog.rows_total", 1506)])
        self.assertEqual(self.cited("61,667,626<!--f:pins.bytes_total-->"), [("pins.bytes_total", 61667626)])

    def test_a_number_written_as_a_word_is_a_citation(self):
        # The record says "all nine pins" and "two rows still", and forcing
        # those into digits would trade the prose for the check.
        self.assertEqual(self.cited("nine<!--f:pins.baselines-->"), [("pins.baselines", 9)])
        self.assertEqual(self.cited("Two<!--f:guard.refusal_reasons-->"), [("guard.refusal_reasons", 2)])

    def test_an_unmarked_number_is_not_a_citation(self):
        # Unmarked is unchecked, on purpose: figures only a live run knows are
        # quoted without a marker, and must not be read as claims about an
        # artifact.
        self.assertEqual(self.cited("577 rows matched on `h1`"), [])

    def test_a_marker_not_attached_to_a_number_is_not_a_citation(self):
        self.assertEqual(self.cited("rows<!--f:catalog.rows_total-->"), [])
        self.assertEqual(self.cited("1,506 <!--f:catalog.rows_total-->"), [])

    def test_a_key_is_lowercase_dotted_and_nothing_else(self):
        self.assertEqual(self.cited("9<!--f:Pins.Baselines-->"), [])
        self.assertEqual(self.cited("9<!--f:pins baselines-->"), [])

    def test_every_citation_in_the_record_parses_to_a_key_and_a_number(self):
        cited = FIGURES.citations(REPOSITORY)
        self.assertTrue(cited, "the record carries no citations at all")
        for key, value in cited:
            self.assertRegex(key, r"^[a-z0-9_.]+$")
            self.assertIsInstance(value, int)


class IndependentFigureTests(unittest.TestCase):
    """Figures recomputed by a route the tool does not take.

    A checker that agrees with itself proves nothing. Each figure here is
    derived from the artifact a second way -- counting raw occurrences rather
    than walking parsed structure, or counting list entries rather than reading
    the count a file declares about itself -- so a defect in the tool's own
    traversal shows up as a disagreement rather than as a consistent wrong
    answer in both places.
    """

    @classmethod
    def setUpClass(cls):
        cls.figures = FIGURES.figures(REPOSITORY)

    def test_catalog_rows_total_counted_off_the_raw_catalog_text(self):
        rows = sum(
            path.read_text(encoding="utf-8").count('"source_locator"')
            for path in sorted((REPOSITORY / FIGURES.CATALOGS).glob("*.json"))
            if path.name != "index.json"
        )
        self.assertEqual(rows, 1506)
        self.assertEqual(self.figures["catalog.rows_total"].value, rows)

    def test_pinned_topics_total_counted_as_manifest_entries(self):
        # The tool sums each manifest's declared `topic_count`; this counts the
        # topics themselves, so a manifest that miscounts its own list would
        # part the two.
        topics = sum(
            len(json.loads(path.read_text(encoding="utf-8"))["topics"])
            for path in sorted((REPOSITORY / FIGURES.MANIFESTS).glob("*-topics.json"))
        )
        self.assertEqual(topics, 4488)
        self.assertEqual(self.figures["pins.topics_total"].value, topics)

    def test_topic_located_rows_counted_off_the_raw_catalog_text(self):
        located = sum(
            path.read_text(encoding="utf-8").count('"source_locator": "topic:')
            for path in sorted((REPOSITORY / FIGURES.CATALOGS).glob("*.json"))
            if path.name != "index.json"
        )
        self.assertEqual(located, 865)
        self.assertEqual(self.figures["catalog.topic_located_total"].value, located)

    def test_the_two_populations_of_rows_account_for_all_of_them(self):
        self.assertEqual(
            self.figures["catalog.topic_located_total"].value
            + self.figures["catalog.not_topic_located_total"].value,
            self.figures["catalog.rows_total"].value,
        )

    def test_publication_locators_and_deliberate_normalizations_account_for_all_rows(self):
        self.assertEqual(636, self.figures["catalog.embedded_located_total"].value)
        self.assertEqual(1501, self.figures["catalog.publication_located_total"].value)
        self.assertEqual(
            self.figures["catalog.publication_located_total"].value
            + self.figures["catalog.roadmap_normalization_total"].value,
            self.figures["catalog.rows_total"].value,
        )

    def test_racf_retained_catalog_classifications_recompute_from_named_entries(self):
        source = REPOSITORY / "conformance/subsystems/racf/racf/operand-dispositions.json"
        book = json.loads(source.read_text(encoding="utf-8"))
        retained = [entry for entry in book["dispositions"] if not entry["applied"]]
        counts = {
            classification: sum(
                entry["emulator_classification"] == classification for entry in retained
            )
            for classification in (
                "implemented",
                "opaque-profile-field",
                "deliberately-unimplemented",
            )
        }
        expected = {
            "implemented": (43, "implemented"),
            "opaque-profile-field": (9, "opaque"),
            "deliberately-unimplemented": (10, "unsupported"),
        }
        for classification, (count, figure) in expected.items():
            self.assertEqual(count, counts[classification])
            self.assertEqual(
                count,
                self.figures[f"racf.{figure}_catalog_only_names"].value,
            )

    def test_racf_publication_classifications_recompute_from_each_named_entry(self):
        source = REPOSITORY / "conformance/subsystems/racf/racf/operand-dispositions.json"
        book = json.loads(source.read_text(encoding="utf-8"))
        populations = (
            (
                "source_only",
                ("implemented", "deliberately_unimplemented", "catalog_gaps"),
                ("implemented", "unsupported", "catalog_gap"),
                (0, 429, 0),
            ),
            (
                "syntax_only",
                (
                    "implemented",
                    "deliberately_unimplemented",
                    "context_only",
                    "catalog_gaps",
                ),
                ("implemented", "unsupported", "context_only", "catalog_gap"),
                (4, 68, 4, 0),
            ),
        )
        for population, fields, figures, expected in populations:
            counts = tuple(
                sum(len(row[population][field]) for row in book["publication_dispositions"])
                for field in fields
            )
            self.assertEqual(expected, counts)
            self.assertEqual(len(figures), len(counts))
            for classification, count in zip(figures, counts):
                self.assertEqual(
                    count,
                    self.figures[f"racf.{classification}_{population}_names"].value,
                )


    def test_every_figure_names_the_file_it_was_read_from(self):
        for key, figure in self.figures.items():
            self.assertTrue(figure.source, f"{key} names no source")
            self.assertIsInstance(figure.value, int)

    def test_the_json_form_claims_no_coverage_credit(self):
        captured = io.StringIO()
        with contextlib.redirect_stdout(captured):
            FIGURES.main(["--format", "json", "--root", str(REPOSITORY)])
        emitted = json.loads(captured.getvalue())
        self.assertEqual(emitted["coverage_credit"], 0)
        self.assertFalse(emitted["retained_in_repository"])
        self.assertEqual(set(emitted["figures"]), set(self.figures))


class RecordAgreementTests(unittest.TestCase):
    """The record as committed agrees with the tree as committed."""

    def test_no_cited_figure_disagrees_with_its_artifact(self):
        wrong, _, _ = FIGURES.check(REPOSITORY)
        self.assertEqual(
            [],
            [
                f"{item.key}: record says {item.quoted:,}, {item.source} says {item.actual:,}"
                for item in wrong
            ],
        )

    def test_no_citation_names_a_figure_no_artifact_reports(self):
        _, unknown, _ = FIGURES.check(REPOSITORY)
        self.assertEqual([], unknown)

    def test_the_committed_record_passes_the_check(self):
        captured = io.StringIO()
        with contextlib.redirect_stdout(captured):
            status = FIGURES.main(["--check", "--root", str(REPOSITORY)])
        self.assertEqual(0, status, captured.getvalue())

    def test_the_figures_the_record_leans_hardest_on_are_cited(self):
        # Not every figure the tool reports has to be quoted -- which ones earn
        # their place is editorial. These are the ones the record's own claims
        # are built out of, so losing the marker off one of them would quietly
        # take it back out of the check.
        cited = {key for key, _ in FIGURES.citations(REPOSITORY)}
        for key in (
            "catalog.rows_total",
            "catalog.topic_located_total",


            "cobol.catalog_forms",
            "cobol.source_formats",
            "racf.source_operands",
            "racf.catalog_only",
            "racf.source_only",
            "ams.source_parameters",
            "jcl.syntax.parameters",
            "pins.topics_total",
        ):
            self.assertIn(key, cited)

    def test_every_figure_that_has_actually_gone_stale_is_cited(self):
        # The ten the last review found wrong, and the value each was wrong
        # about. These are not a guess at what matters: each is a figure a
        # commit in this branch's own range moved while the prose stood still,
        # so an unmarked one is a demonstrated way for this file to go stale
        # again. `was` is recorded so the failure message says which wave it
        # came from rather than only that a marker is gone.
        went_stale = {
            "cobol.catalog_forms": (81, 80),
            "cobol.distinct_catalog_placeholders": (73, 77),
            "cobol.distinct_keywords_missing": (69, 75),
            "cobol.distinct_undefined_placeholders": (6, 2),
            "cobol.rows_with_undefined_placeholders": (10, 8),
            "cobol.forms.set": (7, 6),
            "cobol.missing_keywords.xml_generate": (11, 16),
            "cobol.missing_keywords.set": (10, 11),
            "racf.source_values": (844, 849),
            "racf.source_members": (957, 950),
        }
        known = FIGURES.figures(REPOSITORY)
        cited = {key for key, _ in FIGURES.citations(REPOSITORY)}
        for key, (was, now) in sorted(went_stale.items()):
            with self.subTest(key=key):
                self.assertIn(key, cited, f"{key} went stale at {was} and is cited nowhere")
                self.assertEqual(now, known[key].value, f"{key} is no longer {now}")
                self.assertNotEqual(was, now)


class StaleRecordTests(TreeCase):
    """The tests that fail when the record goes stale, which is the point.

    Each builds a tree carrying this repository's artifacts and a record with
    one thing wrong with it, and asserts `--check` says so. If these ever pass
    without the mutation being detected, the checker has stopped checking.
    """

    def test_one_changed_digit_in_the_record_fails_the_check(self):
        original = record_text()
        wrong, unknown, _ = self.check(restate(original, "catalog.rows_total", "1,507"))
        self.assertEqual([], unknown)
        self.assertEqual(1, len(wrong))
        self.assertEqual("catalog.rows_total", wrong[0].key)
        self.assertEqual(1507, wrong[0].quoted)
        self.assertEqual(1506, wrong[0].actual)

        status, output = self.exit_status(restate(original, "catalog.rows_total", "1,507"))
        self.assertEqual(1, status)
        self.assertIn("quotes catalog.rows_total = 1,507", output)
        self.assertIn("says 1,506", output)

    def test_a_stale_number_of_the_kind_that_actually_went_stale_fails(self):
        # `cobol.catalog_forms` is one of the ten figures commit 397a6b8 moved
        # under a record written before it landed. 81 was right for two commits
        # and is the number four hand-corrections left standing.
        original = record_text()
        wrong, _, _ = self.check(restate(original, "cobol.catalog_forms", "81"))
        self.assertEqual(["cobol.catalog_forms"], [item.key for item in wrong])
        self.assertEqual(81, wrong[0].quoted)
        self.assertEqual(80, wrong[0].actual)

    def test_a_number_written_as_a_word_is_checked_like_any_other(self):
        original = record_text()
        wrong, _, _ = self.check(restate(original, "pins.baselines", "eight"))
        self.assertEqual(["pins.baselines"], [item.key for item in wrong])
        self.assertEqual(8, wrong[0].quoted)
        self.assertEqual(9, wrong[0].actual)

    def test_every_cited_figure_is_individually_checked(self):
        # One mutation per citation, so a key that the checker silently skips
        # cannot hide behind the keys it does check.
        original = record_text()
        for key, value in sorted(set(FIGURES.citations(REPOSITORY))):
            with self.subTest(key=key):
                wrong, unknown, _ = self.check(restate(original, key, str(value + 1)))
                self.assertEqual([], unknown)
                self.assertIn(key, [item.key for item in wrong])

    def test_citing_a_figure_no_artifact_reports_fails_the_check(self):
        status, output = self.exit_status(record_text() + "\n12<!--f:no.such.figure-->\n")
        self.assertEqual(1, status)
        self.assertIn("no.such.figure", output)

    def test_a_figure_the_record_quotes_nowhere_is_reported_and_not_failed(self):
        # The documented asymmetry: which figures the record ought to quote is
        # editorial, so silence is listed rather than failed. Dropping a marker
        # must still be visible, which is why the key is named in the output.
        status, output = self.exit_status(unmark(record_text(), "pins.racroute_bytes"))
        self.assertEqual(0, status)
        self.assertIn("not cited: pins.racroute_bytes", output)


if __name__ == "__main__":
    unittest.main()
