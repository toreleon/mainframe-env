# Publication source probe

Status: **Diagnostic probes; zero coverage credit; not catalog authorities**

Each subsystem catalog cites an IBM publication, and nothing in the repository
checks a catalog against the publication it names. These probes build an
independent machine projection from the published source and diff it against
the committed catalog.

Every probe runs on the pinned edition, and every pinned edition is a set of
IBM Documentation topics. PDF is retired as a source: no baseline pins one, no
catalog row points into one, and no tool left in the tree can read one.
Retrieval is plain HTTPS through `conformance/tools/docs_api.py`, which holds
the two endpoints — the table of contents and the topic content — in one place.
The edge refuses clients by `User-Agent`, and it is the browser string that is
refused (`Mozilla/5.0` and `Python-urllib/3.13` both return 403, `curl/8.7.1`
returns 200), so the client declares the working one explicitly.
`conformance/tools/browser_fetch.py`, which attaches to a Chrome listening on a
debugging port and issues same-origin `fetch` calls from inside the page,
remains the documented fallback; it is roughly forty times slower, which is the
whole reason re-reading every pinned topic is now something anyone will do.

IBM publication bytes are not retained in the repository, and since commit
8389f98 that is enforced by content rather than by path. Four commits have
widened `check_publication_bytes` in `xtask/src/main.rs` since, so what it does
now, rather than what it did:

- It walks **the whole tree**, not `conformance/`. Two<!--f:guard.skipped_directories-->
  directories are skipped by path — `target/` and `.git/` (d79a21c narrowed that
  from every directory *named* `target`) — and `__pycache__` is skipped by name
  at any depth (f87c7d1), because a `.pyc` marshals its source's string constants
  next to one another and so a test written to hold two markers apart compiles to
  bytecode that reads as a served body.
- It refuses a file for one of two<!--f:guard.refusal_reasons--> reasons. Either
  it is **a PDF** — `%PDF-` magic, whatever the file is called — or it carries **a
  topic body served by IBM Documentation**.
- A served body is recognised by the seam IBM's own renderer emits: the topic
  heading closing directly onto the `Last Updated` stamp, `</h1>` onto the `<div
  id="lastModifiedDate">`. Commit 8d82806 made that test positional rather than
  lexical, so it fires **anywhere in the file** — a line of prose in front of the
  body, or the body as the value of a JSON field, no longer defeats it — and `\"`
  is unescaped before the seam is looked for, which is what catches the JSON
  envelope. The older test survives beside it: a file whose first non-whitespace
  byte is `<` and which carries both `topictitle1` and `id="lastModifiedDate"`.

**What still gets past it, named rather than left to be found.** Any
re-serialisation that puts the heading and the stamp apart — pretty-printed
markup, or a JSON encoding with a literal newline between the two elements — and
anything that is not the served bytes at all: a gzipped body, or a
base64-encoded one, matches neither test. This is a backstop and not the control.
The control is `conformance/tools/docs_api.py` (commit 66dc252): retrieved bytes
are written by one function, which refuses any destination inside the tree, and
`python3 conformance/tools/docs_api.py --audit` names every write every
retrieval-capable tool makes and why it is allowed.

Every projection this file describes carries `coverage_credit: 0` and
`retained_in_repository: false`. None of the work recorded here moved a coverage
numerator. `conformance/0.2/evidence/coverage-ledger.json` still reads
`official_compatibility_numerator: 0<!--f:ledger.official_compatibility_numerator-->`
and `generated_catalog_credit: 0<!--f:ledger.generated_catalog_credit-->`, and
the 1,506<!--f:catalog.rows_total--> catalog row identities are unchanged —
`GENERATED_IDENTITY_SET_SHA256` in
`crates/contracts/mainframe-env-host-api/src/generated/official_semantic_identities.rs`
is `sha256:b659a6e1...` as it was before any of it.

## How to check the numbers in this file

This record has gone stale in the same way in each of five waves: a workstream
moves a number, the record was written before it landed, nobody diffs the prose,
and the next wave rewrites it and introduces a fresh error. Four hand-corrections
did not break that, so the numbers are now machine-checked instead.

`conformance/tools/report_probe_figures.py` reads the committed artifacts — the
catalogs, the manifests, the projections, the ledger, and four Rust and Python
source files — and prints every figure this record is allowed to quote, each with
the file and field it came from. It asks IBM nothing and retains nothing.

Every quoted number that the tool reports carries an invisible marker naming the
figure it is: `865<!--f:catalog.topic_located_total-->` renders as `865`, and
`report_probe_figures.py --check` fails if the digits and the artifact disagree,
or if the key names no artifact. `conformance/tools/tests/test_probe_figures.py`
asserts the same thing, so a stale number fails the Python suite rather than
waiting for a reviewer.

Three kinds of number here are deliberately **unmarked, and are not checked**:

- figures only a live run knows — how the 732<!--f:catalog.heading_only_rows-->
  rows that carry neither a chapter number nor a table citation split between the
  served `h1` and a table-of-contents label, or how many topics a re-read reports
  changed;
- figures that describe something this repository no longer has — every count
  attributed to one of the four deleted PDF readers, and every "down from N";
- every figure inside "What a probe finding may and may not become", which is the
  one section not rewritten when the tree moves. Its numbers are corrected in the
  dated subsection beneath it rather than in place, so marking them would make
  the checker demand the edit the section exists to refuse.

The first two are historical or observational and cannot be recomputed from the
tree. Where one appears below it is stated as what it was, not as what is.

`--check` also lists, without failing, every figure the tool reports that this
record quotes nowhere. That asymmetry is on purpose: which figures earn a place
in the prose is editorial, and making silence red would buy agreement by padding
the file with numbers nobody wants to read. A figure quoted **wrong** is a
failure; a figure not quoted is a list entry.

## The pins are reproducible

`conformance/0.2/catalogs/index.json` records, per baseline, the
table-of-contents URL and its sha256, the content URL template
(`?parsebody=true&lang=en` is part of the pin — dropping it moves the MQ topic
from 10,245 to 8,264 bytes), and a manifest under `conformance/0.2/manifests/`
that names every topic the baseline was read from with that topic's own digest.
The baseline's `sha256` is a digest over that ordered topic list, not over a
downloaded file. Since commit 60974e2 the manifests are also gated in Rust
(`xtask/src/topic_manifests.rs`), so a manifest that claims coverage credit,
retains publication bytes, repeats a topic, or names a topic from another
product fails CI rather than a script no one runs.

`conformance/tools/fetch_pinned_sources.py` re-reads all of it: the table of
contents, then every topic the manifest names, then recomputes the manifest
digest from the bytes it got back. Before asking IBM anything it checks each
manifest against itself, because a manifest whose digest no longer follows from
its own topic list is a defect retrieval cannot detect.

**All nine<!--f:pins.baselines--> pins do re-read to their recorded digests — but
not on every run.**
Eight of the nine reproduce on any run. The ninth, db2, is served by an origin
that intermittently returns an older build of a handful of its
832<!--f:pins.topics.db2--> topics, so a
single run will sometimes report a few of them as changed and a re-read of those
same topics returns the pinned bytes. The pin is right and the retrieval is
flaky; that is diagnosed below, and it is why the honest form of this paragraph
is longer than one sentence. Anyone quoting "all nine reproduce" without the
second half will be surprised by the next red run.

| Baseline | Publication | Topics | Bytes | Pin |
|---|---|---|---|---|
| cobol | SC27-8713-04, Enterprise COBOL 6.5 Language Reference | 622<!--f:pins.topics.cobol--> | 8,760,265<!--f:pins.bytes.cobol--> | matches |
| cics | Function codes of EXEC CICS commands, CICS TS 6.x | 1<!--f:pins.topics.cics--> | 265,761<!--f:pins.bytes.cics--> | matches |
| jcl-jes2 | SA23-1385-70, z/OS 3.2 MVS JCL Reference | 1,985<!--f:pins.topics.jcl_jes2--> | 5,419,261<!--f:pins.bytes.jcl_jes2--> | matches |
| dataset-vsam-ams | SC23-6846-70, z/OS 3.2 DFSMS Access Method Services | 516<!--f:pins.topics.dataset_vsam_ams--> | 3,448,164<!--f:pins.bytes.dataset_vsam_ams--> | matches |
| racf-saf | SA23-2292-70, z/OS 3.2 RACF Command Language Reference | 109<!--f:pins.topics.racf_saf--> | 4,011,649<!--f:pins.bytes.racf_saf--> | matches |
| zosmf | SC27-8430-70, z/OSMF Programming Guide | 395<!--f:pins.topics.zosmf--> | 9,761,388<!--f:pins.bytes.zosmf--> | matches |
| db2 | Db2 13 for z/OS SQL Reference | 832<!--f:pins.topics.db2--> | 29,147,264<!--f:pins.bytes.db2--> | matches on re-read; intermittently reported `differs` (see below) |
| ims | Comparing EXEC DLI commands and DL/I calls, IMS 15.6 | 1<!--f:pins.topics.ims--> | 16,046<!--f:pins.bytes.ims--> | matches |
| mq | IBM MQ 9.4 MQI call descriptions | 27<!--f:pins.topics.mq--> | 837,828<!--f:pins.bytes.mq--> | matches |

4,488<!--f:pins.topics_total--> topics and 61,667,626<!--f:pins.bytes_total-->
bytes; both totals are the sum of the nine manifests
in `conformance/0.2/manifests/` and can be recomputed from the tree without
asking IBM anything. All nine tables of contents hash to their recorded
`toc_sha256`. The RACROUTE router-interface topic that RACF pins as a supporting
source is re-read too, at 161,025<!--f:pins.racroute_bytes--> bytes
(`conformance/0.2/catalogs/index.json`, the `racri.htm` supporting entry); no
tool in this repository had ever re-read it, because the old loop read only each
baseline's primary source.

The content endpoint is byte-stable and the rendered DOM is not. That is the
whole reason the pins moved. A rendered capture carries a fresh `lit$<random>$`
nonce and an Adobe `eto_<hex>` nonce on every load, so the three HTML baseline
digests this record used to describe as "not reproducible this way" — cics, ims
and mq, plus the RACROUTE supporting pin — were never reproducible by any
retrieval mode: they were captures of the DOM.

### Db2: the pin is right, the retrieval is intermittently stale

This was recorded here as unresolved. It is resolved, and the resolution is the
narrower of the two possibilities: nothing has drifted.

`fetch_pinned_sources.py --subsystem db2 --no-cache` reported `changed=7`. Each
of those seven topics was then re-read six times, and **42 of 42 reads
reproduced the pinned sha256 exactly**. Twelve *concurrent* reads of one of them,
`db2z_sql_createview`, also returned a single digest — the pinned one — so
request concurrency is not the trigger either. Three further signs point the
same way:

1. Four full runs called 1, 7, 1 and 6 topics changed and **never named the same
   topic twice**. Publication drift is not random per run.
2. Every stale body was **smaller** than its pin, by exactly 10 bytes in five of
   the seven cases.
3. Every stale body carried an **earlier** `Last Updated` than the pin —
   2026-01-07 through 2026-05-19 against a pinned 2026-09-03. Republication moves
   that date forward, not backward. A body stamped 2026-01-07 arriving where a
   2026-09-03 body is pinned is an older build being served, not a newer one
   being published.

The difference between the two builds is typographic. Diffing the pinned
`db2z_sql_createview.html` against the stale one gives two lines: the Last
Updated date, and `SQL statements in Db2&nbsp;for&nbsp;z/OS` against `SQL
statements in Db2 for z/OS` in a cross-reference title. Two `&nbsp;` entities are
exactly the 10 missing bytes. The 2026-09-03 republication inserted non-breaking
spaces; no normative content moved.

The full measurement is not in the repository, because it is publication-derived:
it was written to `$TMPDIR/cobolgrammar/db2-staleness-finding.md` and carries no
coverage credit. What is in the repository is the behaviour it argued for, landed
in commit 16829f8. `fetch_pinned_sources.py` now re-reads a mismatching topic
before recording anything, and classifies by `lastModifiedDate` against the pin:

- the re-read reproduces the pin → **`stale-read`**, which does *not* fail the run;
- served date **earlier** than pinned → `stale-read`, the origin served an older build;
- served date **later** than pinned → **`republished`**, which *does* fail the run
  and is the case a reviewer must actually look at;
- same date, different bytes → **`same-date-different-bytes`**, which *does* fail
  the run: it is the verdict nothing here explains and the alarming one;
- no pair of dates to compare → **`undated-difference`**, which *does* fail the
  run, because a difference that cannot be classified must not be excused.

So three<!--f:tools.failing_resolutions--> of the four resolutions fail and one
does not. `UNEXPLAINED` at
`conformance/tools/fetch_pinned_sources.py:75<!--f:tools.unexplained_line-->` is
the list, and a baseline reads
`differs` when any topic lands in it — or when the manifest digest moves with no
topic reporting a mismatch at all, which is the manifest disagreeing with itself
rather than the origin being slow. A red Db2 run is still possible and still
worth reading, and only `republished` among the three means the publication
moved. Nothing is re-pinned on the strength of any of this; the `Db2 pin — not
re-pinned` entry below stands as written.

An unreachable endpoint reports `skipped` and a partial run reports `sampled`;
neither can be mistaken for a match.

## Row identity checks out everywhere

A publication-located catalog row carries either a
`topic:PATH;topic-id:SLUG;heading:TITLE`, `html-table:`, or `html-link:` locator.
`conformance/tools/verify_topic_locators.py` resolves all three forms against
the live publication. Topic locators check the topic path, tree slug and
heading. Table locators check the complete row or header-cell identity the
catalog cites. Link locators check the reviewed label and target filename as one
pair. This audits row *identity* rather than row *content*. It is also the only
publication check the db2 and zosmf baselines receive.

**The live audit returned `exact` for every publication-located row.** The
audited population is 1,501<!--f:catalog.publication_located_total--> rows:

| Baseline | Rows | Topic locators | Embedded locators | Documented normalization |
|---|---|---|---|---|
| cobol | 173<!--f:catalog.rows.cobol--> | 173<!--f:catalog.topic_located.cobol--> | 0 | 0 |
| jcl-jes2 | 237<!--f:catalog.rows.jcl_jes2--> | 237<!--f:catalog.topic_located.jcl_jes2--> | 0 | 0 |
| dataset-vsam-ams | 36<!--f:catalog.rows.dataset_vsam_ams--> | 31<!--f:catalog.topic_located.dataset_vsam_ams--> | 0 | 5<!--f:catalog.other_located.dataset_vsam_ams--> |
| racf-saf | 48<!--f:catalog.rows.racf_saf--> | 34<!--f:catalog.topic_located.racf_saf--> | 14<!--f:catalog.other_located.racf_saf--> | 0 |
| zosmf | 216<!--f:catalog.rows.zosmf--> | 216<!--f:catalog.topic_located.zosmf--> | 0 | 0 |
| db2 | 174<!--f:catalog.rows.db2--> | 174<!--f:catalog.topic_located.db2--> | 0 | 0 |
| cics | 571<!--f:catalog.rows.cics--> | 0<!--f:catalog.topic_located.cics--> | 571<!--f:catalog.other_located.cics--> | 0 |
| ims | 25<!--f:catalog.rows.ims--> | 0<!--f:catalog.topic_located.ims--> | 25<!--f:catalog.other_located.ims--> | 0 |
| mq | 26<!--f:catalog.rows.mq--> | 0<!--f:catalog.topic_located.mq--> | 26<!--f:catalog.other_located.mq--> | 0 |

The embedded population is 636<!--f:catalog.embedded_located_total--> rows:
all 610<!--f:catalog.html_table_total--> `html-table:` rows and all
26<!--f:catalog.html_link_total--> `html-link:` rows. CICS is resolved by the
complete command/EIBFN/family tuple, IMS by its body-row ordinal plus call and
command, RACROUTE by the unique request-type header cell at the catalog ordinal,
and MQ by the unique call-name/target pair. Two byte-identical `MQMHBUF` anchors
collapse to one semantic link and the report records both occurrences.

The remaining 5<!--f:catalog.roadmap_normalization_total-->
`roadmap-normalization:` rows are not publication-match claims. They are the
deliberate five-row VSAM organization taxonomy documented in
`conformance/0.2/catalogs/README.md`, and the verifier reports each as
`skipped:documented-roadmap-normalization`. Thus
865<!--f:catalog.topic_located_total--> topic rows +
636<!--f:catalog.embedded_located_total--> embedded rows +
5<!--f:catalog.roadmap_normalization_total--> documented normalization rows
accounts for all 1,506<!--f:catalog.rows_total--> rows. Every count in the table
comes from `source_locator` values in `conformance/0.2/catalogs/*.json` and is
recomputed offline. The live audit found no missing, moved, retitled, or
ambiguous topic, table row, header cell, or link; an unreachable source remains
`skipped` rather than becoming a false `missing` finding.

A heading is never the sole discriminator — the topic path is resolved first —
and two of the four ways a heading may match are deliberately looser than string
equality. Both are counted separately in the report rather than folded into
`exact`:

| Matched on | Rows |
|---|---|
| `h1` | 577 |
| `toc-label` | 155 |
| `h1-without-chapter-number` | 113<!--f:catalog.chapter_numbered_headings--> |
| `table-row` | 20<!--f:catalog.table_cited_rows--> |

Those are the topic-locator match names `verify_topic_locators.py` actually emits, and the
last of them was wrong here until 2026-09-08: this table used to name a verdict
`table-cell`, which no tool in the tree has ever emitted. The successful
table verdict is `table-row` — the row is what the ordinal discriminates, and
the cell is only where the label is found. Its failing siblings are named the
same way (`table-row-uncited`, `table-row-ambiguous`, `table-row-absent`,
`table-row-malformed`), so a reviewer grepping the report for the name in this
table now finds them.

The first two rows split the 732<!--f:catalog.heading_only_rows--> rows that
carry neither a chapter number nor a table citation, and that split is the one
figure in this table only a live run knows; the other two are recomputable
offline. 155 rows rest on the tree's own label because the topic heading carries
a word the label does not — Db2 labels a statement `ALLOCATE CURSOR` and heads
its topic `ALLOCATE CURSOR statement`.
113<!--f:catalog.chapter_numbered_headings--> rest on dropping a printed-book
chapter number, because the reviewed label reads `Chapter 4. ALLOCATE` where both
the heading and the tree read `ALLOCATE`; the pattern is exactly `Chapter N.` and
nothing else, and those 113<!--f:catalog.chapter_numbered_headings--> are
recomputable offline as the topic-located rows
whose heading matches `^Chapter \d+\. `
(82<!--f:catalog.chapter_numbered_headings.cobol--> cobol,
31<!--f:catalog.chapter_numbered_headings.dataset_vsam_ams--> dataset-vsam-ams).
Those labels are frozen row identity and cannot be rewritten, so the comparison
gives way instead. The 20<!--f:catalog.table_cited_rows--> JCL statement rows
cite a topic and one row of a shared table together, and are checked against that
row's cells; a topic that no longer carries the named table reports `retitled`,
not a silent pass.

Whatever else is thin about these catalogs, every publication-located inventory
is anchored to the source it cites, and every normalized row is explicitly
identified as such.

## Each publication needs its own reader

The web topics are the artifact of record for every job here. They are what the
digests pin, what the `topic:PATH;topic-id:SLUG;heading:TITLE` locators point
into, and what every reader reads. IBM generates them from the same DITA the
printed book is typeset from, so structure is stated in markup instead of being
inferred from where ink landed on a page.

That is not a preference; it is the finding. All four PDF readers have now been
replaced, and all four corrections were of the same kind: the PDF reader had been
merging or splitting levels the markup states outright.

- Replacing the **COBOL** reader turned 95<!--f:cobol.source_diagrams--> diagrams
  and 59 titles into 83<!--f:cobol.source_formats--> statement formats and
  12<!--f:cobol.source_phrase_fragments--> phrase fragments and disagreed per row
  in 16 of 44<!--f:cobol.rows--> statements.
- Replacing the **AMS** reader turned 889 flat names into
  688<!--f:ams.source_parameters--> parameters and 193<!--f:ams.source_values-->
  values.
- Replacing the **RACF** reader (commit 3f4cbfd, corrected in ff50ae5) took 25
  located commands to 34<!--f:racf.located--> and 534 flat operands to a
  six<!--f:racf.max_nesting_depth-->-level tree of
  751<!--f:racf.source_operands--> operands, 849<!--f:racf.source_values-->
  values and 950<!--f:racf.source_members--> members.
- Replacing the **JCL** reader (commit 4f9aab6) reproduced the same
  204<!--f:jcl.syntax.parameters--> parameters the PDF reader found, in the same
  order.

No publication changed in any of the four; only the source we read it from did.

Each reader reads only pinned topics, at pinned digests. That is checkable
offline: every topic path and sha256 in
`conformance/0.3/generated/cobol-topic-manifest.json` (139<!--f:readers.cobol.topics-->),
`conformance/0.6/generated/ams-topic-manifest.json` (89<!--f:readers.ams.topics-->),
`conformance/0.5/generated/racf-topic-manifest.json` (60<!--f:readers.racf.topics-->) and
`conformance/0.7/generated/jcl-topic-manifest.json` (626<!--f:readers.jcl.topics-->)
appears in the corresponding `conformance/0.2/manifests/` pin at an identical
digest — 139/139<!--f:readers.cobol.topics_in_pin-->,
89/89<!--f:readers.ams.topics_in_pin-->, 60/60<!--f:readers.racf.topics_in_pin-->
and 626/626<!--f:readers.jcl.topics_in_pin-->.

The four readers are genuinely four readers; none of them is a configuration of
another:

- **COBOL** publishes its railroad diagrams twice. The PDF drew them as vector
  art, and the web topics carry DITA markup. The reader takes the markup, because
  the structure is stated there rather than inferred:

      g class='groupseq'
        g class='boxed syntaxvar'      -> identifier-2
        g class=''                     <- optional wrapper
          g class=''                   <- the bypass rail: no text beneath it
          g class='boxed syntaxkwd'    -> ROUNDED

  Nesting, alternation, optionality and the keyword/operand split are all
  explicit. Two rules carry the reader: a group holding a text-free sibling
  marks its remaining children optional, and diagram titles delimit diagrams,
  so a wide diagram split into several `syntaxdiagram-piece` SVGs is joined
  rather than counted twice.

  Statements with several formats publish an overview topic and one child topic
  per format, so the fetcher walks each statement's whole subtree — the `ACCEPT
  statement` topic itself carries no diagram at all.

  This is the same DITA vocabulary the CICS documentation uses, which is what
  makes CICS the nearest future candidate for it. There is no CICS reader in
  this tree to have reused (see "Not yet covered"), and one would not have
  transferred unchanged in either direction: a reader built for `OPTION(kind)`
  operands rejects the bare `syntaxvar` operands COBOL uses.
- **RACF** has no diagrams. Each command topic carries a `Syntax` section and a
  `Parameters` section, and `conformance/0.5/tools/extract_racf_html_syntax.py`
  reads structure only. Every claim is scoped to a `<section>` named by its own
  `<h2 class="sectiontitle">`: operands come from the `dl` under `Parameters`
  and the syntax line from the tables under `Syntax`, so a definition list under
  `Examples` is not syntax. Aliases are searched only inside `section.refsyn`
  and only for a brace group anchored on the command's own keyword — which is
  why `SET` correctly returns none, where the PDF reader returned the operand
  values `SETONLY` and `NOSET`. Each `dt` is read against **its own parent**, not
  against the operand at the root of its branch: a child name the parent already
  offers as an alternative (`DOM(NORMAL | ALL | NONE)`) is a *value*, any other
  child is a *member*. The three compensations the PDF reader needed — locating
  the block by an introduction line, tolerating kerning that split command names
  (`RV ARY`), and recovering segment nesting from line shape — are all gone.
  `RACDCERT` is the one synthesised row: its umbrella topic has no `Parameters`
  section at all, so the row is built from its
  26<!--f:racf.racdcert_function_topics--> function topics with each function's
  own contribution kept beside the union.
- **AMS** puts its parameters in definition lists. A parameter is a top-level
  `dt`; the values it accepts are `dt` entries of a `dl` nested inside its own
  `dd`. Terms carry their argument and their alternation together
  (`INFILE(ddname)|INDATASET(entryname)`), so the reader removes arguments
  before splitting alternations — the other order stops at the first
  parenthesis and loses the second name.
- **JCL** publishes its inventory in the book structure itself: one chapter per
  statement, one topic per parameter. The reader needs no syntax parsing, and the
  one thing the deleted PDF reader got right is the thing easiest to lose on the
  topic tree: the PDF outline was read at depth exactly 1, while the topic
  subtree is deeper — DD alone is 566<!--f:jcl.dd_subtree.nodes--> nodes over four
  levels (1<!--f:jcl.dd_subtree.level_0--> / 75<!--f:jcl.dd_subtree.level_1--> /
  451<!--f:jcl.dd_subtree.level_2--> / 39<!--f:jcl.dd_subtree.level_3-->).
  A descendant walk that admitted anything ending in "parameter" would report 424
  parameters instead of 204<!--f:jcl.syntax.parameters-->, because `Relationship to other parameters`,
  `Examples of the AMP parameter` and `Effect of DCB=dsname parameter` all end in
  the word. `parameters()` therefore takes a chapter's **direct children** and
  nothing below them, and commit 8d0748a made the tests assert that with labels
  that would actually break if the rule were relaxed.

## Results

### JCL — 204 statement parameters, all confirmed

Produced by `conformance/0.7/tools/extract_jcl_html_parameters.py` from the
626<!--f:jcl.topics--> pinned topics it records, and emitted to
`conformance/0.7/generated/jcl-html-parameter-projection.json`. The comparison
confirms a catalog it does not change.

| Unit | catalog | source | shared | only in catalog | only in source |
|---|---|---|---|---|---|
| dd-parameters | 74<!--f:jcl.dd_parameters.catalog_count--> | 74<!--f:jcl.dd_parameters.source_count--> | 74<!--f:jcl.dd_parameters.shared--> | 0 | 0 |
| exec-parameters | 19<!--f:jcl.exec_parameters.catalog_count--> | 19<!--f:jcl.exec_parameters.source_count--> | 19<!--f:jcl.exec_parameters.shared--> | 0 | 0 |
| job-parameters | 35<!--f:jcl.job_parameters.catalog_count--> | 35<!--f:jcl.job_parameters.source_count--> | 35<!--f:jcl.job_parameters.shared--> | 0 | 0 |
| output-parameters | 76<!--f:jcl.output_parameters.catalog_count--> | 76<!--f:jcl.output_parameters.source_count--> | 76<!--f:jcl.output_parameters.shared--> | 0 | 0 |

Every unit reports `ordered_match: true` — the match is by position among a
chapter's children, not merely by set. The statement rosters match the same way:
20<!--f:jcl.jcl_statements.count--> JCL statements and
13<!--f:jcl.jes2_jecl_statements.count--> JES2 JECL statements, each resolved to
the chapter that documents it and cross-checked against the two summary tables
that state the same rosters. 20 + 13 + 74 + 19 + 35 + 76 =
237<!--f:jcl.catalog_rows-->, which is every row in
`conformance/0.2/catalogs/jcl-jes2.json`.

This is the same result the deleted PDF reader produced, ordered-equal, which is
the strongest thing that can be said for either reader. That comparison was made
against what the retired tool actually emitted —
`438dbc6^:conformance/0.7/generated/jcl-pdf-parameter-projection.json` — not
against a re-read of a book nothing in the tree can open, so no PDF was involved
in checking it. On the 3.2 edition the match was exact in every unit. The nine
differences the earlier V2R2 run reported — `DSKEYLBL`, `NULLOVRD`, `ROACCESS`,
`ABDISPCC`, `TVSAMCOM`, `TVSMSG`, `EMAIL`, `GDGBIAS`, and the apostrophe in
`PROGRAMMER'S NAME` — were all edition skew.

A third key, `syntax`, carries per parameter the syntax art and subparameter
terms its `Syntax` and `Subparameter definition` grandchildren publish:
195<!--f:jcl.syntax.with_syntax--> of 204<!--f:jcl.syntax.parameters-->
parameters have syntax art and 178<!--f:jcl.syntax.with_subparameters--> have
subparameters. It is emitted under a
separate key precisely so that a defect there cannot move the inventory counts
above.

### AMS — 688 parameters against a catalog that records none

All 31<!--f:ams.located--> functional commands are located. The publication
documents **688<!--f:ams.source_parameters--> parameters**, plus
**193<!--f:ams.source_values--> values** those parameters accept;
`conformance/0.6/ams/grammar.json` records `keywords: ["ALLOCATE"]` and nothing
else, so its parameter inventory is **zero<!--f:ams.catalog_parameters--> for
every command**. The spread is wide: `ALTER` 59<!--f:ams.parameters.alter-->,
`ALLOCATE` 56<!--f:ams.parameters.allocate-->, `DEFINE CLUSTER`
56<!--f:ams.parameters.define_cluster-->, `REPRO`
48<!--f:ams.parameters.repro-->, `DELETE` 30<!--f:ams.parameters.delete-->,
`BLDINDEX` 14<!--f:ams.parameters.bldindex-->, down to `VERIFY`
2<!--f:ams.parameters.verify-->.

**These numbers correct an earlier PDF-derived run**, which reported 889
parameters. That reader took flush-left headings, and the reference sets values
flush left too, so it counted `SORTMESSAGELEVEL`'s `ALL`, `CRITICAL` and `NONE`
as parameters of `BLDINDEX` — 25 names where the command has
14<!--f:ams.parameters.bldindex-->. The totals
were again close (889 against 688<!--f:ams.source_parameters--> +
193<!--f:ams.source_values--> = 881) because the PDF reader was
merging two levels rather than inventing names, but the split it produced was
wrong.

The empty field in `grammar.json` is real and is not a defect being hidden: it is
a recognition inventory, and the AMS operand contract lives elsewhere. See the
dated correction under "What a probe finding may and may not become" below.

### COBOL — 44 procedure statements

From `conformance/0.3/generated/cobol-grammar-comparison.json`:

| | catalog | source |
|---|---|---|
| Forms | 80<!--f:cobol.catalog_forms--> | — |
| Statement formats | — | 83<!--f:cobol.source_formats--> |
| Phrase fragments | — | 12<!--f:cobol.source_phrase_fragments--> |
| Diagrams (formats + fragments) | — | 95<!--f:cobol.source_diagrams--> |
| Operand naming | 2<!--f:cobol.distinct_undefined_placeholders--> undefined placeholders across 8<!--f:cobol.rows_with_undefined_placeholders--> rows | 80<!--f:cobol.distinct_source_operands--> named operands |

The form gap this section used to describe is now largely closed. Commit 6223be2
wrote the publication's own statement formats into
`conformance/0.3/cobol/language.json`, taking catalog forms from 49 to 81; commit
397a6b8 then held each form to the diagram its own statement draws and settled it
at 80<!--f:cobol.catalog_forms-->, against
83<!--f:cobol.source_formats--> published formats. **Two<!--f:cobol.rows_with_more_source_formats-->
rows still carry more source formats than the catalog has forms**, down from 15:
`ACCEPT` (1<!--f:cobol.forms.accept--> against 2<!--f:cobol.formats.accept-->) and
`SET` (6<!--f:cobol.forms.set--> against 8<!--f:cobol.formats.set-->). No row
carries more forms than the publication has formats — that count is
zero<!--f:cobol.rows_with_more_catalog_forms--> across all
44<!--f:cobol.rows--> rows.

Where a form is deliberately not written, the row records why. There are
12<!--f:cobol.dispositions--> such
`disposition` entries in `language.json` — `accept`, `call`, `evaluate`, `exit`,
`invoke`, `json-generate`, `json-parse`, `read`, `set`, `stop`, `write`,
`xml-generate` — each with a rationale. `EXIT` format 4 (`EXIT FUNCTION`) is
omitted because the reference topic states Enterprise COBOL does not support it;
several others hold keywords out because reserving the word globally would break
programs that parse today.

29<!--f:cobol.rows_with_missing_keywords--> rows still use a keyword that appears
in no catalog form, 75<!--f:cobol.distinct_keywords_missing--> distinct keywords
in total, down from 37 rows and 137 keywords. The remainder concentrates in the
markup-heavy statements: `JSON GENERATE`
19<!--f:cobol.missing_keywords.json_generate-->, `XML GENERATE`
16<!--f:cobol.missing_keywords.xml_generate-->, `JSON PARSE`
12<!--f:cobol.missing_keywords.json_parse-->, `SET`
11<!--f:cobol.missing_keywords.set-->, `START`
7<!--f:cobol.missing_keywords.start-->.

The counts are read against the phrase split. `JSON GENERATE` publishes one
statement format and five phrase diagrams (`when-phrase Format`,
`converting-phrase Format 1`, ...); counting those as formats would overstate
how many ways the statement can be written, so the projection marks each
diagram `format` or `fragment`. That is why
95<!--f:cobol.source_diagrams--> diagrams are 83<!--f:cobol.source_formats-->
formats plus 12<!--f:cobol.source_phrase_fragments--> fragments.

**One metric here moved in a direction that reads like a regression and is not.**
`distinct_catalog_placeholders` went from 33 to
77<!--f:cobol.distinct_catalog_placeholders--> when the new forms landed. The
reference's own operand names are lowercase (`identifier-1`, `literal-2`), so
adopting them raises that count by construction. The metric that answers the
question the old number was standing in for is `distinct_undefined_placeholders`,
added in commit 2112209: placeholders the catalog uses that the publication does
not define for that statement. It is
**2<!--f:cobol.distinct_undefined_placeholders-->, across
8<!--f:cobol.rows_with_undefined_placeholders--> rows**:

- `statements` in `ADD`, `COMPUTE`, `DIVIDE`, `EVALUATE`, `MULTIPLY`, `PERFORM`
  and `SUBTRACT`, where the reference writes `imperative-statement-1`;
- `value` in `STOP`.

Four names this record listed here until 2026-09-08 have left that set, and they
left it in two different ways, which is worth separating because only one of them
is the catalog getting better:

- `objects` and `subjects` in `EVALUATE`, and `fig-con-1` in `XML GENERATE`, went
  because the catalog form was rewritten onto the reference's own operand names.
  `EVALUATE`'s form dropped both and picked up the twenty names the publication
  actually draws (`identifier-1`..`identifier-6`, `condition-1`, `expression-1`,
  `arithmetic-expression-1`.., `literal-1`..); `XML GENERATE`'s `fig-con-1` became
  `generic-suppression-phrase`, which is what the diagram publishes there.
- `data-name-1` in `FREE` went because the *reader* changed, not the catalog. The
  catalog form still writes `data-name-1`; commit 397a6b8 holds each form to the
  diagram its own statement draws, and `FREE`'s diagram — which the previous run
  read as having no operands at all — publishes `data-name-1`. Nothing was
  authored; a source-side blank was filled in.

So 6 across 10 rows falling to 2<!--f:cobol.distinct_undefined_placeholders-->
across 8<!--f:cobol.rows_with_undefined_placeholders--> is three names corrected
in two catalog
forms and one blind spot closed in the reader. None of it is a coverage claim.

**The earlier PDF-derived run should not be quoted.** It reported
95<!--f:cobol.source_diagrams--> diagrams
against only 59 titles. The totals were close by coincidence: the geometry reader
over-split `INSPECT` into nine diagrams where the publication has four and `SORT`
into four where it has two, missed `START`, `UNSTRING` and `GOBACK` entirely, and
read 16 `SET` titles where the publication titles
**8<!--f:cobol.format_titles.set-->**. Per row the two
disagreed in 16 of 44<!--f:cobol.rows--> cases.

The `SET` figure has to be stated that way, because this paragraph exists to say
which reader to believe and the two numbers it used to compare were not
measuring the same thing. `format_titles` for `set` in
`cobol-html-grammar-projection.json` is 8<!--f:cobol.format_titles.set--> —
Format 1 through Format 7 plus
`SET for length of dynamic-length elementary items`. The
6<!--f:cobol.forms.set--> is
`len(catalog_forms)`: what `language.json` records, which is a fact about this
repository and not about the publication. Against the publication the PDF reader
doubled 8 into 16; against the publication the markup reader reads
8<!--f:cobol.format_titles.set-->, and the
catalog is two short of it — the `set` disposition says why. That figure read 7
here until 2026-09-08, when commit 397a6b8 held each form to the diagram its own
statement draws and dropped one of `SET`'s.

### RACF — 34 command families, all located

From `conformance/0.5/generated/racf-html-syntax-projection.json`, built by
`extract_racf_html_syntax.py` from the 60<!--f:racf.topics--> pinned topics it
records (34<!--f:racf.command_topics--> command
topics plus the 26<!--f:racf.racdcert_function_topics--> `RACDCERT` function
topics).

**All 34<!--f:racf.located--> commands are located**, where the deleted PDF
reader reached 25. The
nine it could not reach — `RACDCERT`, `RACMAP`, `RACPRIV`, `RACPRMCK`,
`DISPLAY`, `RESTART`, `SIGNOFF`, `STOP` and `TARGET` — were reader failures, not
publication gaps, and every one of them is now read.

| | |
|---|---|
| Families / located | 34<!--f:racf.families--> / 34<!--f:racf.located--> |
| Catalog operands | 384<!--f:racf.catalog_operands--> |
| Source operands | 751<!--f:racf.source_operands--> |
| Source values | 849<!--f:racf.source_values--> |
| Source members | 950<!--f:racf.source_members--> |
| Only in catalog | 62<!--f:racf.catalog_only--> |
| Only in source | 429<!--f:racf.source_only--> |
| In the syntax line but never defined under Parameters | 76<!--f:racf.syntax_only--> |
| Max nesting depth | 6<!--f:racf.max_nesting_depth--> |

The values and members rows read 844 and 957 here until 2026-09-08. Commit
0dc3024 reported the two populations the earlier overlap total conflated, which
moved both.

The tree is as deep as the publication's list is. Counting
`source_nesting_depth` over the projection's 34<!--f:racf.command_topics--> rows
gives 2<!--f:racf.rows_at_depth_6--> at six<!--f:racf.max_nesting_depth--> levels —
`ALTGROUP` and `ALTUSER` — and **five<!--f:racf.rows_at_depth_5--> at five**:
`ADDGROUP`, `ADDUSER`, `RALTER`,
`SET` and `SETROPTS`. The rest are 4<!--f:racf.rows_at_depth_4--> at four,
3<!--f:racf.rows_at_depth_3--> at three, 16<!--f:racf.rows_at_depth_2--> at two
and 4<!--f:racf.rows_at_depth_1--> at one.
Commit ff50ae5 fixed a collapse that had been flattening it. `syntax_only` —
76<!--f:racf.syntax_only-->
uppercase tokens the `Syntax` table shows that the `Parameters` tree never
reaches — is retained as a distinct projection population and is now explicitly
classified below rather than left as an unanswered review question.

The alias finding is closed. **All 34<!--f:racf.alias_lists_reproduced--> catalog
alias lists reproduce exactly**,
including `SET`, which correctly returns `[]`:
22<!--f:racf.families_with_an_alias--> families carry their documented
abbreviation (`AD`, `AU`, `ALU`, `PE`, `RDEF`, and `PASSWORD`'s `PW` beside the
`PHRASE` it already had), and the other 12<!--f:racf.families_without_an_alias-->
— `DISPLAY`, `RACDCERT`, `RACLINK`,
`RACMAP`, `RACPRIV`, `RACPRMCK`, `RESTART`, `RVARY`, `SET`, `SIGNOFF`, `STOP`,
`TARGET` — genuinely have none.
Five<!--f:racf.rows_without_catalog_operands--> catalog rows still carry no
operands at all
(`DELGROUP`, `DELUSER`, `RDELETE`, `RESTART`, `STOP`).

**Six<!--f:racf.applied_names--> operand names were corrected in the catalog**
(commit 2ea7b0d), each
verified against the topic that publishes it: `ADDGROUP`/`ALTGROUP` `TERMINAL` →
`TERMUACC`, `ALTGROUP` `NOTERMINAL` → `NOTERMUACC`, and `RACPRIV` `LIST`/`OFF`/
`ON` → `WRITEDOWN`. `TERMINAL` occurs zero times in `addgroup.htm` and
`altgrp.htm`, and `racpriv.htm` publishes `WRITEDOWN` as its only `dt` term.

**All 62<!--f:racf.reviewed_catalog_only_names--> retained catalog-only operands
across 19<!--f:racf.remaining_catalog_only_families--> families now carry a named
review, an emulator classification, and a description of the behavior the code
actually provides.** The review is recorded transparently as an automated Codex
review against the pinned topic/projection and the current parser and processor
control flow; it claims no licensed equivalence. The publication-side reasons
remain:
48<!--f:racf.reason.not_published_for_this_command-->
`not-published-for-this-command`,
6<!--f:racf.reason.catalog_fuses_operand_and_value-->
`catalog-fuses-operand-and-value`,
5<!--f:racf.reason.named_only_in_prose_not_in_the_syntax-->
`named-only-in-prose-not-in-the-syntax`,
3<!--f:racf.reason.published_as_a_value_or_subordinate_term-->
`published-as-a-value-or-subordinate-term`. They concentrate in `SETROPTS`
(9<!--f:racf.dispositioned.setropts-->),
`RACMAP` (8<!--f:racf.dispositioned.racmap-->) and `ALTDSD`
(6<!--f:racf.dispositioned.altdsd-->).

Of those 62, 43<!--f:racf.implemented_catalog_only_names--> reach an existing
semantic handler, 9<!--f:racf.opaque_catalog_only_names--> are deliberately
preserved as opaque BASE-profile compatibility fields, and
10<!--f:racf.unsupported_catalog_only_names--> are deliberately unsupported.
The unsupported set is generated into the command descriptors and raises a
structured `UnsupportedCapability` before any state mutation rather than being
accepted as a no-op.

The publication-to-catalog direction is gated as well. All
429<!--f:racf.source_only--> `source_only` names are classified:
0<!--f:racf.implemented_source_only_names--> implemented,
429<!--f:racf.unsupported_source_only_names--> deliberately unsupported, and
the catalog-gap count is 0<!--f:racf.catalog_gap_source_only_names-->. Of the
76<!--f:racf.syntax_only--> names printed in a Syntax section but absent from
the Parameters tree, 4<!--f:racf.implemented_syntax_only_names--> are implemented,
68<!--f:racf.unsupported_syntax_only_names--> are deliberately unsupported in
their recorded command or enclosing-operand contexts, and
4<!--f:racf.context_only_syntax_only_names--> are analysis-only words without an
unambiguous executable context. Arbitrary occurrences of those four remain
ordinary data. The syntax-only catalog-gap count is
0<!--f:racf.catalog_gap_syntax_only_names-->.

`check_operand_dispositions` in `xtask/src/racf_catalog.rs` enforces exact set
equality for `catalog_only`, `source_only`, and `syntax_only`, rejects overlapping
or stale classifications, and requires both catalog-gap sets to remain empty.
The projection still carries zero coverage credit: these decisions prove how
the emulator treats every observed name, not that its modeled behavior is a
licensed RACF differential match.

## Known limitations

- COBOL: `conformance/0.3/generated/cobol-topic-manifest.json` records each
  topic's path and digest so a reviewer can see exactly what was read. All
  139<!--f:readers.cobol.topics-->
  of them are inside the 622<!--f:pins.topics.cobol--> topics
  `conformance/0.2/manifests/cobol-topics.json`
  pins, at identical digests, so the projection is read from the pinned artifact
  rather than from a same-version stand-in. Inside an optional segment every
  branch is reported optional rather than alternative, because none of them can
  be required; alternation is therefore only visible on the main line. `EXIT`
  yields formats 1, 2, 3, 5 and 6, so a format the reference documents without a
  diagram is invisible here.
- RACF: `syntax_only` (76<!--f:racf.syntax_only--> names) cannot separate a
  segment's members from the
  enumerated values of a term the publication does not restate, and the reader
  does not pretend it can. The `RACDCERT` row is synthesised from
  26<!--f:racf.racdcert_function_topics--> function
  topics rather than read from one, so its
  66<!--f:racf.racdcert_source_operands--> source operands are a union across
  functions; each function's own contribution is kept beside the union so a
  reviewer can tell them apart. All 60<!--f:readers.racf.topics--> topics read are
  inside the 109<!--f:pins.topics.racf_saf--> that
  `conformance/0.2/manifests/racf-saf-topics.json` pins, at identical digests.
- AMS: like COBOL, all 89<!--f:readers.ams.topics--> topics in
  `conformance/0.6/generated/ams-topic-manifest.json` are inside the
  516<!--f:pins.topics.dataset_vsam_ams--> that
  `conformance/0.2/manifests/dataset-vsam-ams-topics.json` pins, at identical
  digests. A parameter nested under another (`DEFINE PATH` documents `NAME` and
  `PATHENTRY` inside `PATH(...)`) is reported as a value, which is faithful to
  the reference's own nesting but means the parameter count is per level rather
  than per command. Unlike the RACF reader, the AMS reader flattens every depth
  below the first into one list, so it cannot answer which parameter a nested
  name belongs to.
- JCL: the inventory is a chapter's direct children only, so a parameter
  documented outside a `... parameter` topic at that depth is not seen — the same
  class of blind spot the PDF reader had, for the same reason, and the price of
  not admitting the 220 further names a descendant walk would report (424 rather
  than 204<!--f:jcl.syntax.parameters-->, from cross-reference and example topics
  that also end in the word
  "parameter"). All 626<!--f:readers.jcl.topics--> topics read are inside the
  1,985<!--f:pins.topics.jcl_jes2--> that
  `conformance/0.2/manifests/jcl-jes2-topics.json` pins, at identical digests.
- Locator audit: a heading is never the only discriminator, and it must not
  become one. 52<!--f:catalog.repeated_headings--> of the
  865<!--f:catalog.topic_located_total--> rows carry a heading that repeats inside
  their own
  book — 43<!--f:catalog.repeated_headings.jcl_jes2--> in jcl-jes2,
  6<!--f:catalog.repeated_headings.cobol--> in cobol,
  3<!--f:catalog.repeated_headings.zosmf--> in zosmf — so a rewrite that chose a
  row's topic by matching heading text could permute them arbitrarily and still
  pass every count-based check. Every row is resolved by its topic path first.
  The three z/OSMF rows `:0075`, `:0141` and `:0153` are the sharp case: all
  three are labelled and headed `Error reporting categories`, and their only
  discriminator is now the topic path and topic-id, which differ
  (`izuprog_API_StgMgt_...`, `IZUHPINFO_API_RESTFILES_...`,
  `IZUHPINFO_API_...`).

## What a probe finding may and may not become

Only one gap found here was safe to close mechanically. The distinction is
whether the projection is catalog-grade, not whether the gap is real:

- **RACF aliases — fixed.** Each one is a single unambiguous token read from
  the command's own syntax block, cross-checked against the abbreviations RACF
  operators actually type. Adding them changes only which selectors the parser
  accepts; `conformance/spec/v1/spec.json` is unchanged, so no obligation or
  coverage claim moves.
- **AMS parameters — ready, pending a contract decision.** This was previously
  recorded as unfixable because the projection conflated parameters with
  values. That was a property of the PDF reader, not of the publication: the
  web topics separate the two by `dl` nesting, and the projection now does too.
  What is left is not a data question but a scope one. `ams/grammar.json` has
  `additionalProperties: false` and no parameter field, so carrying parameters
  means changing the schema, `AmsGrammarEntry`, the xtask renderer and the
  generated digest. It would also be inert today: the AMS grammar is used only
  to recognize a command from its leading keywords
  (`crates/apps/mainframe-env-batch/src/ams.rs`), and nothing validates
  parameters. Whether the AMS contract should grow that surface is a decision
  for the owner, and the data is ready either way.
- **RACF operands — not promoted.** Nine commands are still unlocated and some
  blocks may close early, so the 89 catalog-only names cannot be separated from
  reader failures. Accepting an operand the command processor does not
  implement is also worse than rejecting it.
- **COBOL forms — not promoted.** The catalog forms are authored prose
  sketches, not a mechanical projection of the diagrams. Closing the
  49-against-95 gap means writing 46 forms, which is review work.
- **Db2 pin — not re-pinned.** Re-pinning means re-extracting and re-reviewing
  174 rows against the new edition.

### Correction to the section above — 2026-09-07

The section above is the written form of the zero-coverage-credit rule. It is
left uncorrected in place and corrected here instead, and — because this is the
one part of the file that is deliberately not rewritten — its numbers carry no
`f:` markers and are not checked. Read them as of the date beside them; the
current values are in this subsection and in the sections above.

**What its preservation actually amounts to, since three commit messages have
overstated it.** Those messages say the section is "byte-identical to de596ee".
That is not true and cannot be: de596ee is a `.gitignore` commit, and neither
this file nor its predecessor existed at it. The real history is shorter than
the claim and still worth having. The section was written in commit 9fc473f, in
this file's predecessor `docs/research/pdf-grammar-source-probe.md`. Commit
a85c1b6 rewrote one of its five bullets — AMS parameters, from "not promoted" to
"ready, pending a contract decision", when the markup reader stopped conflating
parameters with values. Since a85c1b6 the section body has not changed a byte,
including across the rename in 517892f. It is not, however, the whole of the
section any more: this dated subsection is appended under the same heading.

**The rule itself is unchanged and all five rulings still stand.** Nothing found
by a probe has been promoted into a catalog or a contract on the strength of the
probe, and `official_compatibility_numerator` and `generated_catalog_credit` are
both still `0`. What has changed is the factual premises three of the rulings
were argued from.

- **"nothing validates parameters" (AMS) is false.**
  `ams_operand_allowed` at
  `crates/apps/mainframe-env-batch/src/service.rs:6202<!--f:ams.allowlist_line-->`
  is a per-command allowlist of **126<!--f:ams.allowlist_names--> distinct
  operand names**, 84<!--f:ams.allowlist_base_names--> of them the
  base set shared by `ALLOCATE`, `DEFINE CLUSTER`, `DEFINE NONVSAM`, `DEFINE
  ALTERNATEINDEX` and `ALTER` and the rest declared per command.
  `unimplemented_ams_operand` at `:6080<!--f:ams.unimplemented_line-->` scans
  every top-level term and its caller at
  `:3050<!--f:ams.unimplemented_caller_line-->` raises `UnsupportedCapability` on
  capability `ams-operand` before any
  effect runs. The ruling — that `grammar.json` stays a recognition inventory and
  should not grow a parameter field — is *strengthened* by this, not weakened:
  feeding the 688<!--f:ams.source_parameters--> projected parameters to
  `ams_operand_allowed` would convert a
  loud `UnsupportedCapability` into a silently accepted operand for every name the
  emulator has no effect for. The contract that does exist is documented in
  `docs/architecture/DATASET-VSAM-AMS.md`, which names both halves of it — the
  typed effects in `conformance/0.6/inventory/dataset-programming-surface.json`
  and the accepted spellings in `ams_operand_allowed` — and says why the accepted
  set is deliberately narrower than the publication's and is not nested inside it
  in either direction.
- **"Nine commands are still unlocated" (RACF operands) is false.** All
  34<!--f:racf.located--> are
  located by the topic reader, and the catalog-only count is
  62<!--f:racf.catalog_only-->, not 89. The
  ruling stands and its second sentence is now the whole of its reasoning:
  accepting an operand the command processor does not implement is worse than
  rejecting it. The 62<!--f:racf.remaining_catalog_only_names--> are
  dispositioned by name in
  `conformance/0.5/racf/operand-dispositions.json` and gated by
  `check_operand_dispositions` in `xtask/src/racf_catalog.rs`; six further names
  were corrected outright.
- **"Closing the 49-against-95 gap means writing 46 forms" is superseded.** The
  gap was 49 forms against 83<!--f:cobol.source_formats--> published statement
  formats — 95<!--f:cobol.source_diagrams--> was the diagram
  count, which includes 12<!--f:cobol.source_phrase_fragments--> phrase
  fragments. The forms were written as review
  work, exactly as the ruling requires, and the catalog now carries
  80<!--f:cobol.catalog_forms--> forms with
  12<!--f:cobol.dispositions--> recorded dispositions. It briefly carried 81:
  commit 6223be2 wrote them and commit 397a6b8, which landed after this
  correction was first written, held each form to the diagram its own statement
  draws and settled it at 80. The ruling that forms are authored rather than
  projected is unchanged; nothing was promoted mechanically.

## Not yet covered

| Subsystem | State |
|---|---|
| Db2 | 832<!--f:pins.topics.db2--> topics pinned and all 174<!--f:catalog.rows.db2--> rows resolve. No syntax reader; statement syntax is published as the same DITA railroad markup COBOL uses, so the COBOL reader is the nearest starting point. |
| z/OSMF | REST families rather than a command language. All 216<!--f:catalog.rows.zosmf--> rows resolve; a syntax projection does not apply without a different comparison model. |
| CICS, IMS, MQ | All three pin topics and all three reproduce. The locator audit resolves every `html-table:` and `html-link:` identity exactly: 571<!--f:catalog.rows.cics--> CICS rows, 25<!--f:catalog.rows.ims--> IMS rows and 26<!--f:catalog.rows.mq--> MQ rows. IMS and MQ have no syntax reader. CICS now has a bounded accepted file/UOW pilot under `conformance/0.9`, but it is not a reader or implementation for the complete 263-command application API. The CICS rows cite the EIBFN function-code table in `dfha8mf.html`, which proves the inventory and still says nothing about each command's complete syntax. 0.9.0 remains proposed. |

The remaining publication-analysis gaps, in order of size:

1. **No syntax reader for Db2, CICS, IMS or MQ**, which between them carry 796
   of the 1,506<!--f:catalog.rows_total--> catalog rows
   (174<!--f:catalog.rows.db2--> + 571<!--f:catalog.rows.cics--> +
   25<!--f:catalog.rows.ims--> + 26<!--f:catalog.rows.mq-->); z/OSMF's
   216<!--f:catalog.rows.zosmf--> need a
   different comparison model rather than a reader. Their row identities are
   audited; syntax depth is the remaining gap.
2. **75<!--f:cobol.distinct_keywords_missing--> COBOL keywords across
   29<!--f:cobol.rows_with_missing_keywords--> rows** that appear in no catalog
   form. This read 69 until 2026-09-08; commit 397a6b8 held each form to the
   diagram its own statement draws, which moved the forms the keywords are
   counted against.

## Corrections

This record is the probes' current state, not a dated snapshot, so it is
rewritten when what it describes changes. The log below says what changed and
when, so a reader who quoted an older revision can tell whether the number they
quoted still stands. Dated *delivery* records are the opposite — those are
corrected beneath the original sentence, never edited — and one such correction,
in `docs/delivery/coverage-versions/status/0.2.0.md`, covers the same migration.

**2026-09-07 (commits f41ab73..517892f) — PDF is retired as a source, and this
file was rewritten to say so.** It was previously named
`pdf-grammar-source-probe.md`. What changed, and what a reader of the earlier
revision should stop quoting:

- **The pins are not PDFs, and were never verifiable as three of them claimed.**
  All nine baselines now pin a manifest of IBM Documentation topics under
  `conformance/0.2/manifests/`, read from the content endpoint
  (`?parsebody=true&lang=en`). The earlier "The pins are reproducible" section
  carried a six-row table of PDF downloads that no baseline cites any more, and
  said the four HTML pins "need a recorded retrieval method before they can be
  re-verified". They now have one. The stronger statement is that the old HTML
  pins had no such method available: they were captures of the rendered DOM,
  which carries a fresh nonce per load, so no retrieval mode we can identify
  would ever have returned those bytes twice.
- **The locator audit is a different tool, over more rows.**
  `conformance/tools/verify_outline_locators.py` is deleted;
  `conformance/tools/verify_topic_locators.py` resolves the
  `topic:PATH;topic-id:SLUG;heading:TITLE` locators the rows carry now. It covers
  865<!--f:catalog.topic_located_total--> rows rather than 845, because the
  20<!--f:catalog.table_cited_rows--> JCL table rows it used to report as
  "not an outline locator" now cite a topic and a table together. The old known
  limitation — that a repeated outline title matched any of its pages, so a row
  pointing at the wrong occurrence still read exact — is gone with the page
  numbers.
- **Read commit 438dbc6's message with care.** It says "All nine reproduce: 4,488
  topics re-read, 0 changed, 0 unreachable". That was a true report of one run and
  is not a property of the endpoint: the Db2 book does not re-read that way every
  time. Anyone reading `git log` alone will come away with a stronger claim than
  the tree supports. The stale-read finding above is the accurate version, and
  commit 16829f8 is what makes a future run's red readable.

**2026-09-07 (commits a6674d0..118639d, then 8389f98..2112209) — the JCL and RACF
readers were replaced, not merely deleted, and three sets of numbers here were
superseded.** The revision before this one said, in three places, that the PDF
readers were "deleted with no replacement in this wave" and that the JCL and RACF
results "cannot currently be re-derived" / "can no longer be re-derived from
anything in the tree". That under-claimed the tree: both replacements landed on
this same branch, in commits 3f4cbfd (RACF) and 4f9aab6 (JCL), and both results
are now regenerable from pinned topics. The JCL and RACF sections above are no
longer marked provisional, and the "Not yet covered" row that listed them is
gone.

- **RACF numbers superseded.** Stop quoting "25 of 34 commands were located",
  "Source operands 534", "Only in catalog 89" and "nine commands were not
  located". The current figures are 34<!--f:racf.located-->/34<!--f:racf.families-->
  located, 751<!--f:racf.source_operands--> source operands,
  849<!--f:racf.source_values-->
  values, 950<!--f:racf.source_members--> members,
  62<!--f:racf.catalog_only--> catalog-only, 429<!--f:racf.source_only-->
  source-only, and they are read from
  `conformance/0.5/generated/racf-html-syntax-projection.json`. The nine
  "unlocated" commands were a property of the PDF reader. The values and members
  figures read 844 and 957 in this entry until 2026-09-08; commit 0dc3024 moved
  them.
- **COBOL numbers superseded.** Stop quoting "96 diagrams", "18 rows carry more
  source formats", "139 distinct keywords" and "Phrase fragments 13". The
  publication publishes 95<!--f:cobol.source_diagrams--> diagrams =
  83<!--f:cobol.source_formats--> formats + 12<!--f:cobol.source_phrase_fragments-->
  fragments; 2<!--f:cobol.rows_with_more_source_formats--> rows carry
  more source formats than the catalog has forms;
  75<!--f:cobol.distinct_keywords_missing--> distinct keywords across
  29<!--f:cobol.rows_with_missing_keywords-->
  rows are missing. The form gap the section described is largely closed —
  catalog forms went 49 → 81 in commit 6223be2, and 397a6b8 then settled them at
  80<!--f:cobol.catalog_forms--> — and
  `distinct_catalog_placeholders` rising 33 → 77<!--f:cobol.distinct_catalog_placeholders-->
  is a consequence of adopting the
  reference's own lowercase operand names, not a regression;
  `distinct_undefined_placeholders`, added in 2112209, is the metric that
  answers the question, and it is now
  2<!--f:cobol.distinct_undefined_placeholders-->. Four of the figures in this
  entry — the keyword count, the placeholder count, the form count and the
  undefined-placeholder count — were quoted here as 69, 73, 81 and 6 until
  2026-09-08, which is exactly the staleness this entry exists to warn about;
  commit 397a6b8 moved all four.
- **A stale CICS sentence is removed.** The revision before this one said "The
  CICS reader does not transfer unchanged" in one section and, in another, that
  no CICS reader exists anywhere in the tree. The second was right. The sentence
  was a survival from a retracted claim about a "0.9 CICS DITA reader"; commit
  3aefd72 took back the claim and missed this sentence. What is true, and is now
  stated where the sentence was, is that a reader built for `OPTION(kind)`
  operands would not read COBOL's bare `syntaxvar` operands — a reason a future
  CICS reader will be its own reader, not a fact about one that exists.
- **The Db2 cause is no longer unresolved.** The revision before this one said
  "What the cause is remains unresolved" and that no re-verification run had been
  reproducible enough to say. It has since been measured: 42 of 42 re-reads of
  the seven reported-changed topics reproduce the pin, every stale body is
  smaller than its pin and carries an earlier `Last Updated`, and republication
  moves that date forward. The pin is correct and the origin is intermittently
  stale. `fetch_pinned_sources.py` now classifies accordingly (commit 16829f8).
  **Three<!--f:tools.failing_resolutions--> of its four resolutions fail a run,
  not one.**
  `UNEXPLAINED = (REPUBLISHED, SAME_DATE, UNDATED)` at
  `conformance/tools/fetch_pinned_sources.py:75<!--f:tools.unexplained_line-->` is
  what `topics_unexplained`
  counts and what decides `differs`: `republished`, `same-date-different-bytes`
  and `undated-difference` each fail; only `stale-read` does not. The
  db2-staleness finding recommended failing on `republished` alone, and the code
  went further on purpose — a same date over different bytes is the case neither
  story explains and the one a reviewer most needs to see, so saying it is
  benign is the opposite of what it is. A `differs` also stands when the manifest
  digest moves with no topic reporting a mismatch. Nothing is re-pinned.
- **What did not move.** No coverage claim. Every projection named here carries
  `coverage_credit: 0`; `conformance/0.2/evidence/coverage-ledger.json` still
  reads
  `official_compatibility_numerator: 0<!--f:ledger.official_compatibility_numerator-->`
  and `generated_catalog_credit: 0<!--f:ledger.generated_catalog_credit-->`;
  the 1,506<!--f:catalog.rows_total--> row identities and
  `GENERATED_IDENTITY_SET_SHA256` are unchanged.
  Three waves of work deliberately moved no coverage number, and this record must
  not be read as though they did.
