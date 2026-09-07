from __future__ import annotations

import importlib.util
import unittest
from pathlib import Path
from types import ModuleType

TOOLS = Path(__file__).resolve().parents[1]


def load_module(name: str) -> ModuleType:
    path = TOOLS / f"{name}.py"
    spec = importlib.util.spec_from_file_location(name, path)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


AMS = load_module("extract_ams_html_parameters")
FETCH = load_module("fetch_ams_topics")

BLDINDEX = """
<dl class="parml">
  <dt class="pt dlterm">INFILE(<span>ddname</span>)|INDATASET(<span>entryname</span>)</dt>
  <dd>names the DD statement.
    <dl class="parml">
      <dt>INFILE(ddname)</dt><dd>is the DD statement.</dd>
      <dt>INDATASET(entryname)</dt><dd>is the data set.</dd>
    </dl>
  </dd>
  <dt class="pt dlterm">SORTMESSAGELEVEL(<span>option</span>)</dt>
  <dd>controls messages.
    <dl class="parml">
      <dt>ALL</dt><dd>prints all.</dd>
      <dt>CRITICAL</dt><dd>prints critical.</dd>
      <dt>NONE</dt><dd>prints none.</dd>
    </dl>
  </dd>
  <dt class="pt dlterm">ERASE|NOERASE</dt><dd>overwrites or not.</dd>
</dl>
"""


class NameTests(unittest.TestCase):
    def test_an_argument_is_stripped_from_a_term(self) -> None:
        self.assertEqual(AMS.names("CATALOG(catname)"), ["CATALOG"])

    def test_an_alternation_yields_every_name(self) -> None:
        self.assertEqual(AMS.names("ERASE|NOERASE"), ["ERASE", "NOERASE"])

    def test_arguments_are_removed_before_the_alternation_is_split(self) -> None:
        # Splitting first would stop at the paren and lose the second name.
        self.assertEqual(
            AMS.names("INFILE(ddname)|INDATASET(entryname)"), ["INFILE", "INDATASET"]
        )

    def test_an_alternation_inside_an_argument_is_not_a_parameter(self) -> None:
        self.assertEqual(AMS.names("EXCLUDE(entryname|mask)"), ["EXCLUDE"])

    def test_reference_brackets_are_discarded(self) -> None:
        self.assertEqual(AMS.names("[EXCLUDE(entryname)"), ["EXCLUDE"])

    def test_a_lowercase_placeholder_is_not_a_parameter(self) -> None:
        self.assertEqual(AMS.names("entryname"), [])

    def test_a_single_letter_is_not_a_parameter(self) -> None:
        self.assertEqual(AMS.names("U"), [])


class NestingTests(unittest.TestCase):
    def test_top_level_terms_are_parameters(self) -> None:
        top, _ = AMS.parameters([BLDINDEX])
        self.assertEqual(
            top,
            ["INFILE", "INDATASET", "SORTMESSAGELEVEL", "ERASE", "NOERASE"],
        )

    def test_nested_terms_are_values_not_parameters(self) -> None:
        _, nested = AMS.parameters([BLDINDEX])
        self.assertEqual(nested, ["ALL", "CRITICAL", "NONE"])

    def test_a_name_repeated_beneath_its_own_term_stays_a_parameter(self) -> None:
        top, nested = AMS.parameters([BLDINDEX])
        self.assertIn("INFILE", top)
        self.assertNotIn("INFILE", nested)

    def test_a_topic_without_definition_lists_contributes_nothing(self) -> None:
        self.assertEqual(AMS.parameters(["<p>prose only</p>"]), ([], []))

    def test_terms_from_several_topics_are_merged_without_duplicates(self) -> None:
        top, _ = AMS.parameters([BLDINDEX, BLDINDEX])
        self.assertEqual(len(top), len(set(top)))


class TopicSelectionTests(unittest.TestCase):
    def test_only_parameter_topics_are_collected(self) -> None:
        chapter = {
            "label": "DELETE",
            "href": "delet.htm",
            "topics": [
                {"label": "DELETE Parameters", "href": "a.htm", "topics": [
                    {"label": "Required Parameters", "href": "b.htm"},
                    {"label": "Optional Parameters", "href": "c.htm"},
                ]},
                {"label": "DELETE Examples", "href": "d.htm", "topics": [
                    {"label": "Delete a Page Space: Example 14", "href": "e.htm"}
                ]},
            ],
        }
        self.assertEqual(
            [label for label, _ in FETCH.parameter_topics(chapter)],
            ["DELETE Parameters", "Required Parameters", "Optional Parameters"],
        )

    def test_the_singular_heading_is_collected_too(self) -> None:
        chapter = {"label": "VERIFY", "href": "v.htm", "topics": [
            {"label": "Required Parameter", "href": "w.htm"}
        ]}
        self.assertEqual(
            [label for label, _ in FETCH.parameter_topics(chapter)], ["Required Parameter"]
        )


if __name__ == "__main__":
    unittest.main()
