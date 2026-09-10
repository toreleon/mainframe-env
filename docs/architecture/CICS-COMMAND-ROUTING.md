# CICS command descriptor and semantic-family routing

Status: **Implemented**
Owner: **CICS provider maintainers**
Scope: **typed CICS operation descriptors, generated routing families, and provider semantic modules**
Applies from: **mainframe-env 0.9.0 development**

## Authorities

The readable command authority is
[`conformance/0.9/cics/command-descriptors.json`](../../conformance/0.9/cics/command-descriptors.json),
validated by
[`cics-command-descriptors.schema.json`](../../conformance/0.9/schemas/cics-command-descriptors.schema.json).
It keeps two collections deliberately separate:

- `application_catalog` projects all 263 mandatory `api-commands` row IDs,
  official labels and two-byte EIB function codes. It fixes
  `automatic_registration=false` and `generated_coverage_credit=0`; presence
  does not imply grammar, option, condition, handler or execution support.
- `runtime` retains the 25 existing typed `CicsOperation` bindings: 23 API
  operations and two explicit SPI compatibility operations. Each continues to
  record mutation classification and one of the seven reviewed runtime
  families below.

The application projection is bound to official-catalog SHA-256
`fccd2a8e5cc24dd08aeb32754daf14ed80e9f1b20b5d9e762a1b0cfe429ceeba`.
Its formatting-independent, domain-separated identity-set digest is
`a18057164564781563252d53586a8afbc401f2783950db456b3b98177fc60b94`.
EIBFN is metadata rather than identity: the 263 rows contain 258 distinct
codes, with four reviewed shared-code groups.

`python3 -B tools/generate_cics_descriptors.py` deterministically writes the
unchanged provider runtime descriptor module and the additive host-API
application identity table. Use `--check` to compare both outputs without
writing. The JSON Schema instance check, freshness check and module-boundary
guard run in
`cargo xtask architecture-fast --check`; hand-editing generated Rust or
changing the catalog without regeneration fails the gate.

Source preparation remains a separate, non-executable authority. The pinned
CICS TS 6.x table of contents is reduced to the 345 immediate children of the
command-summary node in
[`command-summary-topics.json`](../../conformance/0.9/cics/command-summary-topics.json).
[`application-api-sources-a-map.json`](../../conformance/0.9/cics/application-api-sources-a-map.json)
then accounts for catalog rows `0001`–`0088` without adding grammar, options,
conditions, family ownership, handlers or registration. The map has 88 row
dispositions, 117 row-to-page edges and 109 unique pages. It records
`CICSMESSAGE`, `DUMP` and `ENTER TRACEID` as source gaps because the pinned
command-summary node contains no matching command page; similarly named
commands with different EIBFN values are not aliases. Five supplemental HTML
topics close these map-stage gaps for the later independent verification.

`python3 -B tools/generate_cics_source_map.py --toc -` reads the exact pinned
TOC bytes from standard input and writes only the bounded metadata projection
and zero-credit candidate map. `--check` is offline by default and verifies
their domain-separated digests, exact catalog binding, row order, page
multiplicity, source gaps and canonical rendering. Supplying `--toc -` with
`--check` additionally compares the committed projection to the external
source. Raw TOC and topic bodies remain outside the repository. The map's
candidate status cannot register behavior or grant semantic, execution or
differential credit.

The next source boundary is
[`application-api-sources-a-corpus.json`](../../conformance/0.9/cics/application-api-sources-a-corpus.json).
It binds the accepted map to a 173-topic
[`topic manifest`](../../conformance/0.9/manifests/cics-application-api-sources-a-topics.json):
109 mapped command pages, 58 non-recursive one-hop context pages and six
explicit manual context topics. The command-summary body is excluded as
navigation because its pinned TOC projection is already authoritative. Current
HTML topics retain the legacy EIBFN identities and provide replacement context,
but do not turn `DUMP TRANSACTION`, `ENTER TRACENUM` or `MONITOR` into aliases
for the legacy commands. The manual set includes the current `DFHCMP` module
topic, which identifies the old `ENTER TRACEID` monitoring path without
supplying the command's complete semantics.

`python3 -B conformance/0.9/tools/fetch_cics_application_sources.py --check`
validates committed metadata offline. Supplying `--cache /ibm-docs/topic-cache`
additionally verifies all topic and TOC bodies, re-derives the one-hop closure
and never opens the browser. Generation fetches fresh content-endpoint bytes
through `conformance/tools/browser_fetch.py` before publishing verified,
content-addressed cache entries. Corpus membership alone makes no syntax,
option, condition, applicability or handler decision. The zero-credit
[`browser verification receipt`](../../conformance/0.9/cics/application-api-sources-a-browser-verification.json)
binds a direct Chrome reproduction of all 173 topic identities to the manifest;
it is source-freshness evidence, not semantic authority.

The completed projection child is defined by
[`application-api-sources-a-extraction.json`](../../conformance/0.9/cics/application-api-sources-a-extraction.json)
and materialized as
[`cics-application-api-sources-a-candidates.json`](../../conformance/0.9/generated/cics-application-api-sources-a-candidates.json).
The exact plan file is SHA-256
`09f49b1e424c04ec7b0c0c4a1d696e3e623a6b2969428378cbeb05f87b57b147`,
its domain-separated logical digest is
`86e9b93a1c0bf1caac892eb8d4ab75e956458a77c2683ec58b74de8fa4925b05`,
and the projector implementation is SHA-256
`3bed782bbe61b061ada5ce3bd6b44fab842c4b9415bb7c88c071082c4cd139a5`.
The projection digest is
`e7739cf6e364f347bb67a870e7f5de6c0024661a00a9bf41d2ebd5a1ecb4aa02`.

The projector accounts for all 88 rows through 440 ordered dimension records
and 6,033 candidates. Its source-shape inventory contains 111 syntax diagrams,
112 SVG parts, 895 top-level option terms, and 344 top-level conditions. The
resulting dimension states are 422 projected, 14 declared absent and four
source-backed not applicable, with no source-gap, unmatched or conflicting
dimension. Candidate values contain only bounded symbolic metadata,
structural locators, and fragment hashes; publication text, semantic
acceptance, handler registration, and coverage credit remain outside this
authority.

The no-cache projector check validates the committed identities, shape,
canonical encoding, and logical digest. The cache-backed form additionally
regenerates the complete output from all digest-pinned HTML bodies and compares
it byte for byte. Neither form fetches documentation or accepts its own
interpretation; the independent `sources-a-review` child verifies objective
source facts, while the later command contract owns semantics.

That review child has a compact
[`receipt`](../../conformance/0.9/cics/application-api-sources-a-review.json),
[`schema`](../../conformance/0.9/schemas/cics-source-review.schema.json),
[`independent verifier`](../../conformance/0.9/tools/verify_cics_application_sources.py),
and [`checker`](../../conformance/0.9/tools/review_cics_application_sources.py).
It derives expected facts from cached HTML before reading the projection,
auto-accepts clear facts, and records only dynamic counts, digests and exact
ambiguity IDs. The ordinary check fails on missing source facts, reprojection
findings or mismatches; no manual approval bypass exists. The workflow grants
zero coverage, semantic or differential credit. It passes for all 88 rows and
6,033 candidates across 178 topics: structural coverage is 2,017/2,017, with
5,672 verified candidates, 361 bounded candidate ambiguities, two bounded
target-equivalence issues and no blocking finding.

A focused official-IBM HTML search through the user's Chrome found no exact
current-product `DUMP` or `ENTER TRACEID` command page. No PDF was used. Five
supplemental HTML topics provide authority-bounded objective evidence, so source
verification accepts both rows while preserving their two target-equivalence
issues; it does not inherit `DUMP TRANSACTION`, `ENTER TRACENUM` or `MONITOR`
semantics. The zero-credit 263-row contract scaffold now exists in 88/88/87
batches: `sources-a` facts are populated, `sources-b` and `sources-c` are
explicitly not projected, and 23 existing runtime commands remain distinct from
240 unimplemented rows. Next, project `sources-b`/`sources-c`, enrich the
contract, and implement vertical family slices; release, deployment and
Docker-in-Docker remain out of scope.

## Frozen families

| Family | Owns |
|---|---|
| `task-control` | task context, HANDLE state, ASSIGN, RETRIEVE, ABEND, and pseudo-conversation RETURN |
| `time` | ASKTIME clock acquisition and FORMATTIME conversion |
| `program-control` | program inquiry, LINK, and XCTL |
| `terminal-control` | BMS and text send/receive behavior |
| `file-control` | file status, keyed I/O, and browse behavior |
| `queue-control` | transient-data queue writes |
| `recovery` | SYNCPOINT coordination, rollback, and subsystem unit-of-work completion |

These families apply only to the 25-row runtime collection.
`CicsService::invoke_run` selects the generated runtime descriptor first and routes on
its family. Each family has a real reviewed implementation module under
`crates/providers/mainframe-env-cics/src/handlers/`; no command is dispatched by
an ad hoc keyword match in the service monolith. The service retains the shared
session/run state, authorization boundary, durable compare-and-swap primitives,
common condition/response machinery, and bounded codecs used across families.
The accepted `retention.rs` sibling owns provider-lifecycle codecs and
dependency descriptions; it is deliberately outside the command-family layer
and cannot become an alternate dispatch path.

The compiler/runtime boundary is governed by
[ADR-0011](../decisions/0011-typed-language-hir-and-semantic-ir.md). A migrated
COBOL CICS family resolves static command identity, options, resource bindings,
and output destinations before execution, then emits the existing owned typed
request through the execution coordinator. The provider never parses COBOL HIR
or source syntax, and the migration cannot introduce a parallel CICS provider,
store, unit-of-work protocol, or condition authority.

## Change contract

Adding an identity to the application projection requires a reviewed change to
the pinned official denominator and regeneration. It never updates
`CicsOperation`, dispatch or coverage by itself.

Adding or changing an executable typed CICS command requires one reviewable
change that:

1. updates the readable descriptor catalog and its official row binding;
2. regenerates the Rust descriptor module;
3. adds semantics to the named family module, keeping it below the hard
   1,200-production-line limit; and
4. adds focused condition, authorization, durability, and recovery tests
   appropriate to the operation.

A new semantic family changes the accepted module inventory and requires an ADR
amendment. Generated descriptors never contain behavior, and handler modules
are never generator-owned. [ADR-0010](../decisions/0010-rust-module-review-budgets.md)
records the hard limits and exact legacy ceiling policy.

## Verification

```bash
python3 -B tools/generate_cics_descriptors.py --check
python3 -B tools/generate_cics_source_map.py --check
python3 -B conformance/0.9/tools/fetch_cics_application_sources.py --check
python3 -B conformance/0.9/tools/extract_cics_application_sources.py --check
python3 -B conformance/0.9/tools/extract_cics_application_sources.py --cache /ibm-docs/topic-cache --check
python3 -B conformance/0.9/tools/review_cics_application_sources.py --check
python3 -B -m unittest tools.tests.test_cics_descriptors tools.tests.test_cics_source_map tools.tests.test_module_boundaries
cargo test -p mainframe-env-host-api -p mainframe-env-cics --all-features --locked
cargo xtask architecture-fast --check
```
