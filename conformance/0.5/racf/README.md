# RACF/SAF 0.5 conformance inputs

The command, RACROUTE, and supplied-class catalogs in this directory project
into the shared Conformance IR. They do not define a second RACF authority.

## Reading the publication

`command-language.json` is reviewed row content and is never written by a tool.
What a tool produces is the evidence beside it: `../tools/fetch_racf_topics.py`
selects the 60 topics the RACF Command Language Reference publishes the command
syntax in, and `../tools/extract_racf_html_syntax.py` projects what they say
into `../generated/racf-html-syntax-projection.json`.

Both read documentation topics, never the PDF. The reference draws no railroad
diagrams: a command's syntax is a `role="presentation"` table under a
`<h2 class="sectiontitle">Syntax</h2>`, and its operands are a `<dl class="parml">`
under `Parameters`, where an operand is a top-level `dt` and everything it
accepts or contains is a `dt` of a `dl` nested inside its `dd`. Reading the
typeset page instead meant recovering that nesting from indentation, which
reached 25 of the 34 families and promoted a segment's members to top level
whenever the segment opened inline.

Selection is by href and the predicate is written out in the fetcher's module
docstring, because a reviewer has to be able to arrive at the same 60 topics
without running anything. 34 command families is an immutable denominator, so a
tree that does not publish exactly 34 stops the tool rather than producing a
short manifest.

To reproduce, writing publication bytes outside the repository:

```text
python3 conformance/0.5/tools/fetch_racf_topics.py \
  --destination "$TMPDIR/racf/topics" \
  --manifest conformance/0.5/generated/racf-topic-manifest.json
python3 conformance/0.5/tools/extract_racf_html_syntax.py \
  --topics "$TMPDIR/racf/topics" \
  --manifest conformance/0.5/generated/racf-topic-manifest.json \
  --catalog conformance/0.5/racf/command-language.json \
  --output conformance/0.5/generated/racf-html-syntax-projection.json
python3 -m unittest discover -s conformance/0.5/tools/tests \
  -t conformance/0.5/tools/tests
```

The manifest records each topic's digest against
`conformance/0.2/manifests/racf-saf-topics.json`, so the projection states which
pinned bytes it rests on rather than asking a reviewer to take it on trust.

The projection is a review input. It carries `coverage_credit: 0`, it is not a
normative catalog, and its `catalog_only` and `syntax_only` lists are questions
for review — a name the catalog carries that the Parameters tree does not reach,
and a keyword the syntax table draws that the Parameters tree does not define.
Neither is a defect claim, and neither may be applied to `command-language.json`
without a reviewed disposition per name.

The 0.5 development gate also runs a bounded, table-driven, pure-state
reference simulation over the same 34 command and 14 RACROUTE row identities.
It is intentionally independent of the production provider and has explicit
unknowns for installation exits, cryptographic material, undocumented database
internals, and RRSF transport timing. Its property, metamorphic, and mutant
tests provide development assurance only: they do not create a campaign file,
an oracle receipt, a differential binding, or licensed credit.

Licensed differential is enabled only when an approved adapter installs
`licensed-oracle.json` at this path. The file must validate against
`../schemas/racf-oracle-campaign.schema.json`, cover exactly the frozen 34
command and 14 RACROUTE rows, identify IBM z/OS 3.2 RACF/SAF, assert that the
environment is licensed, carry the approved adapter and licensed-environment
identity prefixes, declare external licensed execution and a fresh
release-certify campaign, and contain no credential, key, token, certificate,
or MFA material. Simulated, modeled, documentation-derived, historical, and
current-product origins fail before registry projection. Absence of the file
means `differential=pending`.

Each case binds the exact generated differential fixture digest and carries a
normalized JSON observation produced by the licensed adapter:

- command: `surface`, `keyword`, `status`, and redacted `records`;
- RACROUTE: `surface`, `keyword`, `status`, `states`, and redacted `result`.

The product driver executes the same selected route on the current candidate
and requires JSON equality with the adapter observation. Only then does it emit
the campaign file digest through the shared runner's oracle-receipt field. The
shared cache identity already binds candidate, catalog, spec, fixture, oracle,
environment, shard, row, gate, and obligation identities.

The campaign is an imported receipt, not a hand-authored expectation. After an
approved adapter supplies it, run:

```text
cargo xtask racf-catalog
cargo xtask spec --check
cargo xtask conformance --subsystem racf-saf --gate differential
```

Do not use official documentation, local product output, or a synthetic fixture
as a substitute for the licensed IBM execution receipt. Under the user-approved
2026-09-01 scoped completion policy, its absence keeps 0.5 at
`differential=0/48 pending`; the real campaign remains mandatory at the 0.17
`release-certify` hard gate.
