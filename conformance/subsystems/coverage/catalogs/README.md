# Catalog source locators

`source_locator` identifies the publication evidence for one frozen catalog
row. `conformance/tools/verify_topic_locators.py` resolves the four conventions
that make a publication claim:

- `topic:` checks the topic path, table-of-contents id and reviewed heading. A
  locator may additionally name one table row.
- `html-table:` checks one complete table identity. CICS uses the reviewed
  command label with its EIBFN and family cells; IMS also cites the source row
  ordinal and command cell; RACROUTE resolves a request type to one unique
  header cell of its cross-reference matrix.
- `html-link:` checks the MQ call label and target filename as one pair.
  Repeated byte-identical anchors count as one semantic link because the
  `mqi-calls-unique` unit deliberately deduplicates them.

`roadmap-normalization:vsam-primary-organizations` is deliberately different.
The five rows in `dataset-vsam-ams.json` are the normalized VSAM organization
taxonomy frozen in
`conformance/roadmap/ibm-official-coverage-roadmap.json`; they do not claim that
one IBM topic publishes a five-row inventory. The verifier keeps all five
visible as `skipped:documented-roadmap-normalization`, with zero coverage
credit. They must not be reported as publication matches unless they are
replaced by locators to evidence that actually enumerates each row.

An unreachable source body always yields `skipped:endpoint-unreachable`.
Absence or disagreement is emitted with the same `missing`, `moved`, and
`retitled` vocabulary as topic locators, so loss of network access cannot be
mistaken for publication drift.
