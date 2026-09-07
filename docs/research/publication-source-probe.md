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
8389f98 that is enforced by content rather than by path: `check_publication_bytes`
in `xtask/src/main.rs` refuses any file whose leading bytes are `%PDF-`,
wherever it sits.

Every projection this file describes carries `coverage_credit: 0` and
`retained_in_repository: false`. None of the work recorded here moved a coverage
numerator. `conformance/0.2/evidence/coverage-ledger.json` still reads
`official_compatibility_numerator: 0` and `generated_catalog_credit: 0`, and the
1,506 catalog row identities are unchanged —
`GENERATED_IDENTITY_SET_SHA256` in
`crates/contracts/mainframe-env-host-api/src/generated/official_semantic_identities.rs`
is `sha256:b659a6e1...` as it was before any of it.

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

**All nine pins do re-read to their recorded digests — but not on every run.**
Eight of the nine reproduce on any run. The ninth, db2, is served by an origin
that intermittently returns an older build of a handful of its 832 topics, so a
single run will sometimes report a few of them as changed and a re-read of those
same topics returns the pinned bytes. The pin is right and the retrieval is
flaky; that is diagnosed below, and it is why the honest form of this paragraph
is longer than one sentence. Anyone quoting "all nine reproduce" without the
second half will be surprised by the next red run.

| Baseline | Publication | Topics | Bytes | Pin |
|---|---|---|---|---|
| cobol | SC27-8713-04, Enterprise COBOL 6.5 Language Reference | 622 | 8,760,265 | matches |
| cics | Function codes of EXEC CICS commands, CICS TS 6.x | 1 | 265,761 | matches |
| jcl-jes2 | SA23-1385-70, z/OS 3.2 MVS JCL Reference | 1,985 | 5,419,261 | matches |
| dataset-vsam-ams | SC23-6846-70, z/OS 3.2 DFSMS Access Method Services | 516 | 3,448,164 | matches |
| racf-saf | SA23-2292-70, z/OS 3.2 RACF Command Language Reference | 109 | 4,011,649 | matches |
| zosmf | SC27-8430-70, z/OSMF Programming Guide | 395 | 9,761,388 | matches |
| db2 | Db2 13 for z/OS SQL Reference | 832 | 29,147,264 | matches on re-read; intermittently reported `differs` (see below) |
| ims | Comparing EXEC DLI commands and DL/I calls, IMS 15.6 | 1 | 16,046 | matches |
| mq | IBM MQ 9.4 MQI call descriptions | 27 | 837,828 | matches |

4,488 topics and 61,667,626 bytes; both totals are the sum of the nine manifests
in `conformance/0.2/manifests/` and can be recomputed from the tree without
asking IBM anything. All nine tables of contents hash to their recorded
`toc_sha256`. The RACROUTE router-interface topic that RACF pins as a supporting
source is re-read too, at 161,025 bytes
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

So three of the four resolutions fail and one does not. `UNEXPLAINED` at
`conformance/tools/fetch_pinned_sources.py:76` is the list, and a baseline reads
`differs` when any topic lands in it — or when the manifest digest moves with no
topic reporting a mismatch at all, which is the manifest disagreeing with itself
rather than the origin being slow. A red Db2 run is still possible and still
worth reading, and only `republished` among the three means the publication
moved. Nothing is re-pinned on the strength of any of this; the `Db2 pin — not
re-pinned` entry below stands as written.

An unreachable endpoint reports `skipped` and a partial run reports `sampled`;
neither can be mistaken for a match.

## Row identity checks out everywhere

A topic-located catalog row carries a `topic:PATH;topic-id:SLUG;heading:TITLE`
locator, and `conformance/tools/verify_topic_locators.py` resolves each one
against the live publication, checking three things: the topic path answers, the
tree publishes that topic-id slug at that path, and the heading still reads what
the row says it reads. This audits row *identity* rather than row *content*. It
is the only check the two baselines with no syntax reader of their own — db2 and
zosmf — get at all.

**865 of 865 topic-located rows resolve exactly:**

| Baseline | Rows | Exact | Not a topic locator |
|---|---|---|---|
| cobol | 173 | 173 | 0 |
| jcl-jes2 | 237 | 237 | 0 |
| dataset-vsam-ams | 36 | 31 | 5 (`roadmap-normalization:`) |
| racf-saf | 48 | 34 | 14 (`html-table:`) |
| zosmf | 216 | 216 | 0 |
| db2 | 174 | 174 | 0 |
| cics | 571 | 0 | 571 (`html-table:`) |
| ims | 25 | 0 | 25 (`html-table:`) |
| mq | 26 | 0 | 26 (`html-link:`) |

No topic was missing and none had moved. The 641 rows reported as "not a topic
locator" are exactly the 610 `html-table:`, 26 `html-link:` and 5
`roadmap-normalization:` rows the catalogs carry; each is reported with that
reason rather than dropped, so 865 + 641 accounts for all 1,506 rows. Every
figure in that table is a count of `source_locator` values in
`conformance/0.2/catalogs/*.json` and can be recomputed offline.

A heading is never the sole discriminator — the topic path is resolved first —
and two of the four ways a heading may match are deliberately looser than string
equality. Both are counted separately in the report rather than folded into
`exact`:

| Matched on | Rows |
|---|---|
| `h1` | 577 |
| `toc-label` | 155 |
| `h1-without-chapter-number` | 113 |
| `table-cell` | 20 |

155 rows rest on the tree's own label because the topic heading carries a word
the label does not — Db2 labels a statement `ALLOCATE CURSOR` and heads its
topic `ALLOCATE CURSOR statement`. 113 rest on dropping a printed-book chapter
number, because the reviewed label reads `Chapter 4. ALLOCATE` where both the
heading and the tree read `ALLOCATE`; the pattern is exactly `Chapter N.` and
nothing else, and those 113 are recomputable offline as the topic-located rows
whose heading matches `^Chapter \d+\. ` (82 cobol, 31 dataset-vsam-ams). Those
labels are frozen row identity and cannot be rewritten, so the comparison gives
way instead. The 20 JCL statement rows cite a topic and one row of a shared table
together, and are checked against that table's cells; a topic that no longer
carries the named table reports `retitled`, not a silent pass.

Whatever else is thin about these catalogs, their inventories are anchored to
the publications they cite.

## Each publication needs its own reader

The web topics are the artifact of record for every job here. They are what the
digests pin, what the `topic:PATH;topic-id:SLUG;heading:TITLE` locators point
into, and what every reader reads. IBM generates them from the same DITA the
printed book is typeset from, so structure is stated in markup instead of being
inferred from where ink landed on a page.

That is not a preference; it is the finding. All four PDF readers have now been
replaced, and all four corrections were of the same kind: the PDF reader had been
merging or splitting levels the markup states outright.

- Replacing the **COBOL** reader turned 95 diagrams and 59 titles into 83
  statement formats and 12 phrase fragments and disagreed per row in 16 of 44
  statements.
- Replacing the **AMS** reader turned 889 flat names into 688 parameters and 193
  values.
- Replacing the **RACF** reader (commit 3f4cbfd, corrected in ff50ae5) took 25
  located commands to 34 and 534 flat operands to a six-level tree of 751
  operands, 844 values and 957 members.
- Replacing the **JCL** reader (commit 4f9aab6) reproduced the same 204
  parameters the PDF reader found, in the same order.

No publication changed in any of the four; only the source we read it from did.

Each reader reads only pinned topics, at pinned digests. That is checkable
offline: every topic path and sha256 in
`conformance/0.3/generated/cobol-topic-manifest.json` (139),
`conformance/0.6/generated/ams-topic-manifest.json` (89),
`conformance/0.5/generated/racf-topic-manifest.json` (60) and
`conformance/0.7/generated/jcl-topic-manifest.json` (626) appears in the
corresponding `conformance/0.2/manifests/` pin at an identical digest — 139/139,
89/89, 60/60 and 626/626.

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
  section at all, so the row is built from its 26 function topics with each
  function's own contribution kept beside the union.
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
  subtree is deeper — DD alone is 566 nodes over four levels (1 / 75 / 451 / 39).
  A descendant walk that admitted anything ending in "parameter" would report 424
  parameters instead of 204, because `Relationship to other parameters`,
  `Examples of the AMP parameter` and `Effect of DCB=dsname parameter` all end in
  the word. `parameters()` therefore takes a chapter's **direct children** and
  nothing below them, and commit 8d0748a made the tests assert that with labels
  that would actually break if the rule were relaxed.

## Results

### JCL — 204 statement parameters, all confirmed

Produced by `conformance/0.7/tools/extract_jcl_html_parameters.py` from the 626
pinned topics it records, and emitted to
`conformance/0.7/generated/jcl-html-parameter-projection.json`. The comparison
confirms a catalog it does not change.

| Unit | catalog | source | shared | only in catalog | only in source |
|---|---|---|---|---|---|
| dd-parameters | 74 | 74 | 74 | 0 | 0 |
| exec-parameters | 19 | 19 | 19 | 0 | 0 |
| job-parameters | 35 | 35 | 35 | 0 | 0 |
| output-parameters | 76 | 76 | 76 | 0 | 0 |

Every unit reports `ordered_match: true` — the match is by position among a
chapter's children, not merely by set. The statement rosters match the same way:
20 JCL statements and 13 JES2 JECL statements, each resolved to the chapter that
documents it and cross-checked against the two summary tables that state the same
rosters. 20 + 13 + 74 + 19 + 35 + 76 = 237, which is every row in
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
terms its `Syntax` and `Subparameter definition` grandchildren publish: 195 of
204 parameters have syntax art and 178 have subparameters. It is emitted under a
separate key precisely so that a defect there cannot move the inventory counts
above.

### AMS — 688 parameters against a catalog that records none

All 31 functional commands are located. The publication documents **688
parameters**, plus **193 values** those parameters accept;
`conformance/0.6/ams/grammar.json` records `keywords: ["ALLOCATE"]` and nothing
else, so its parameter inventory is **zero for every command**. The spread is
wide: `ALTER` 59, `ALLOCATE` 56, `DEFINE CLUSTER` 56, `DELETE` 30, `BLDINDEX` 14,
down to `VERIFY` 2.

**These numbers correct an earlier PDF-derived run**, which reported 889
parameters. That reader took flush-left headings, and the reference sets values
flush left too, so it counted `SORTMESSAGELEVEL`'s `ALL`, `CRITICAL` and `NONE`
as parameters of `BLDINDEX` — 25 names where the command has 14. The totals
were again close (889 against 688 + 193 = 881) because the PDF reader was
merging two levels rather than inventing names, but the split it produced was
wrong.

The empty field in `grammar.json` is real and is not a defect being hidden: it is
a recognition inventory, and the AMS operand contract lives elsewhere. See the
dated correction under "What a probe finding may and may not become" below.

### COBOL — 44 procedure statements

From `conformance/0.3/generated/cobol-grammar-comparison.json`:

| | catalog | source |
|---|---|---|
| Forms | 81 | — |
| Statement formats | — | 83 |
| Phrase fragments | — | 12 |
| Diagrams (formats + fragments) | — | 95 |
| Operand naming | 6 undefined placeholders across 10 rows | 80 named operands |

The form gap this section used to describe is now largely closed. Commit 6223be2
wrote the publication's own statement formats into
`conformance/0.3/cobol/language.json`, taking catalog forms from 49 to 81 against
83 published formats. **Two rows still carry more source formats than the catalog
has forms**, down from 15: `ACCEPT` (1 against 2) and `SET` (7 against 8). No row
carries more forms than the publication has formats.

Where a form is deliberately not written, the row records why. There are 12 such
`disposition` entries in `language.json` — `accept`, `call`, `evaluate`, `exit`,
`invoke`, `json-generate`, `json-parse`, `read`, `set`, `stop`, `write`,
`xml-generate` — each with a rationale. `EXIT` format 4 (`EXIT FUNCTION`) is
omitted because the reference topic states Enterprise COBOL does not support it;
several others hold keywords out because reserving the word globally would break
programs that parse today.

29 rows still use a keyword that appears in no catalog form, 69 distinct keywords
in total, down from 37 rows and 137 keywords. The remainder concentrates in the
markup-heavy statements: `JSON GENERATE` 19, `JSON PARSE` 12, `XML GENERATE` 11,
`SET` 10, `START` 7.

The counts are read against the phrase split. `JSON GENERATE` publishes one
statement format and five phrase diagrams (`when-phrase Format`,
`converting-phrase Format 1`, ...); counting those as formats would overstate
how many ways the statement can be written, so the projection marks each
diagram `format` or `fragment`. That is why 95 diagrams are 83 formats plus 12
fragments.

**One metric here moved in a direction that reads like a regression and is not.**
`distinct_catalog_placeholders` went from 33 to 73 when the new forms landed. The
reference's own operand names are lowercase (`identifier-1`, `literal-2`), so
adopting them raises that count by construction. The metric that answers the
question the old number was standing in for is `distinct_undefined_placeholders`,
added in commit 2112209: placeholders the catalog uses that the publication does
not define for that statement. It is **6, across 10 rows**:

- `statements` in `ADD`, `COMPUTE`, `DIVIDE`, `EVALUATE`, `MULTIPLY`, `PERFORM`
  and `SUBTRACT`, where the reference writes `imperative-statement-1`;
- `objects` and `subjects` in `EVALUATE`;
- `value` in `STOP`;
- `data-name-1` in `FREE` and `fig-con-1` in `XML GENERATE` — both real reference
  operand names, but not published for those two statements.

**The earlier PDF-derived run should not be quoted.** It reported 95 diagrams
against only 59 titles. The totals were close by coincidence: the geometry reader
over-split `INSPECT` into nine diagrams where the publication has four and `SORT`
into four where it has two, missed `START`, `UNSTRING` and `GOBACK` entirely, and
read 16 `SET` titles where the publication titles **8**. Per row the two
disagreed in 16 of 44 cases.

The `SET` figure has to be stated that way, because this paragraph exists to say
which reader to believe and the two numbers it used to compare were not
measuring the same thing. `format_titles` for `set` in
`cobol-html-grammar-projection.json` is 8 — Format 1 through Format 7 plus
`SET for length of dynamic-length elementary items`. The 7 is
`len(catalog_forms)`: what `language.json` records, which is a fact about this
repository and not about the publication. Against the publication the PDF reader
doubled 8 into 16; against the publication the markup reader reads 8, and the
catalog is one short of it — the `set` disposition says why.

### RACF — 34 command families, all located

From `conformance/0.5/generated/racf-html-syntax-projection.json`, built by
`extract_racf_html_syntax.py` from the 60 pinned topics it records (34 command
topics plus the 26 `RACDCERT` function topics).

**All 34 commands are located**, where the deleted PDF reader reached 25. The
nine it could not reach — `RACDCERT`, `RACMAP`, `RACPRIV`, `RACPRMCK`,
`DISPLAY`, `RESTART`, `SIGNOFF`, `STOP` and `TARGET` — were reader failures, not
publication gaps, and every one of them is now read.

| | |
|---|---|
| Families / located | 34 / 34 |
| Catalog operands | 384 |
| Source operands | 751 |
| Source values | 844 |
| Source members | 957 |
| Only in catalog | 62 |
| Only in source | 429 |
| In the syntax line but never defined under Parameters | 76 |
| Max nesting depth | 6 |

The tree is as deep as the publication's list is. Counting
`source_nesting_depth` over the projection's 34 rows gives 2 at six levels —
`ALTGROUP` and `ALTUSER` — and **five at five**: `ADDGROUP`, `ADDUSER`, `RALTER`,
`SET` and `SETROPTS`. The rest are 4 at four, 3 at three, 16 at two and 4 at one.
Commit ff50ae5 fixed a collapse that had been flattening it. `syntax_only` — 76
uppercase tokens the `Syntax` table shows that the `Parameters` tree never
reaches — is a reviewer's question, not a defect claim.

The alias finding is closed. **All 34 catalog alias lists reproduce exactly**,
including `SET`, which correctly returns `[]`: 22 families carry their documented
abbreviation (`AD`, `AU`, `ALU`, `PE`, `RDEF`, and `PASSWORD`'s `PW` beside the
`PHRASE` it already had), and the other 12 — `DISPLAY`, `RACDCERT`, `RACLINK`,
`RACMAP`, `RACPRIV`, `RACPRMCK`, `RESTART`, `RVARY`, `SET`, `SIGNOFF`, `STOP`,
`TARGET` — genuinely have none. Five catalog rows still carry no operands at all
(`DELGROUP`, `DELUSER`, `RDELETE`, `RESTART`, `STOP`).

**Six operand names were corrected in the catalog** (commit 2ea7b0d), each
verified against the topic that publishes it: `ADDGROUP`/`ALTGROUP` `TERMINAL` →
`TERMUACC`, `ALTGROUP` `NOTERMINAL` → `NOTERMUACC`, and `RACPRIV` `LIST`/`OFF`/
`ON` → `WRITEDOWN`. `TERMINAL` occurs zero times in `addgroup.htm` and
`altgrp.htm`, and `racpriv.htm` publishes `WRITEDOWN` as its only `dt` term.

**62 catalog operand names across 19 families are dispositioned but not
applied**, and that is the largest open item this record has. They are recorded
one by one in `conformance/0.5/racf/operand-dispositions.json` with a reason:
48 `not-published-for-this-command`, 6 `catalog-fuses-operand-and-value`, 5
`named-only-in-prose-not-in-the-syntax`, 3
`published-as-a-value-or-subordinate-term`. They concentrate in `SETROPTS` (9),
`RACMAP` (8) and `ALTDSD` (6).

That file *is* gated. `check_operand_dispositions` in `xtask/src/racf_catalog.rs`
enforces set equality between the projection's `catalog_only` names and the
undeferred dispositions, so an unpublished operand cannot be added to the catalog
without being dispositioned, and a disposition cannot outlive the name it
explains. But the gate is one-sided in two ways a reader should hold onto:

1. A *reason* is not a correction. 62 names the publication does not publish are
   still in the catalog, accepted with an explanation attached.
2. Nothing gates the other direction at all. 429 operand names the publication
   publishes and the catalog does not carry are reported by the projection and
   checked by nothing.

## Known limitations

- COBOL: `conformance/0.3/generated/cobol-topic-manifest.json` records each
  topic's path and digest so a reviewer can see exactly what was read. All 139
  of them are inside the 622 topics `conformance/0.2/manifests/cobol-topics.json`
  pins, at identical digests, so the projection is read from the pinned artifact
  rather than from a same-version stand-in. Inside an optional segment every
  branch is reported optional rather than alternative, because none of them can
  be required; alternation is therefore only visible on the main line. `EXIT`
  yields formats 1, 2, 3, 5 and 6, so a format the reference documents without a
  diagram is invisible here.
- RACF: `syntax_only` (76 names) cannot separate a segment's members from the
  enumerated values of a term the publication does not restate, and the reader
  does not pretend it can. The `RACDCERT` row is synthesised from 26 function
  topics rather than read from one, so its 66 source operands are a union across
  functions; each function's own contribution is kept beside the union so a
  reviewer can tell them apart. All 60 topics read are inside the 109 that
  `conformance/0.2/manifests/racf-saf-topics.json` pins, at identical digests.
- AMS: like COBOL, all 89 topics in
  `conformance/0.6/generated/ams-topic-manifest.json` are inside the 516 that
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
  than 204, from cross-reference and example topics that also end in the word
  "parameter"). All 626 topics read are inside the 1,985 that
  `conformance/0.2/manifests/jcl-jes2-topics.json` pins, at identical digests.
- Locator audit: a heading is never the only discriminator, and it must not
  become one. 52 of the 865 rows carry a heading that repeats inside their own
  book — 43 in jcl-jes2, 6 in cobol, 3 in zosmf — so a rewrite that chose a
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

The section above is the written form of the zero-coverage-credit rule and is
kept byte-identical to its original wording, so three of its claims are corrected
here rather than edited in place. **The rule itself is unchanged and all five
rulings still stand.** What has changed is the factual premises three of them
were argued from.

- **"nothing validates parameters" (AMS) is false.**
  `ams_operand_allowed` at `crates/apps/mainframe-env-batch/src/service.rs:6169`
  is a per-command allowlist of **126 distinct operand names**, 84 of them the
  base set shared by `ALLOCATE`, `DEFINE CLUSTER`, `DEFINE NONVSAM`, `DEFINE
  ALTERNATEINDEX` and `ALTER` and the rest declared per command.
  `unimplemented_ams_operand` at `:6047` scans every top-level term and its caller
  at `:2925` raises `UnsupportedCapability` on capability `ams-operand` before any
  effect runs. The ruling — that `grammar.json` stays a recognition inventory and
  should not grow a parameter field — is *strengthened* by this, not weakened:
  feeding the 688 projected parameters to `ams_operand_allowed` would convert a
  loud `UnsupportedCapability` into a silently accepted operand for every name the
  emulator has no effect for. The contract that does exist is documented in
  `docs/architecture/DATASET-VSAM-AMS.md`, which names both halves of it — the
  typed effects in `conformance/0.6/inventory/dataset-programming-surface.json`
  and the accepted spellings in `ams_operand_allowed` — and says why the accepted
  set is deliberately narrower than the publication's and is not nested inside it
  in either direction.
- **"Nine commands are still unlocated" (RACF operands) is false.** All 34 are
  located by the topic reader, and the catalog-only count is 62, not 89. The
  ruling stands and its second sentence is now the whole of its reasoning:
  accepting an operand the command processor does not implement is worse than
  rejecting it. The 62 are dispositioned by name in
  `conformance/0.5/racf/operand-dispositions.json` and gated by
  `check_operand_dispositions` in `xtask/src/racf_catalog.rs`; six further names
  were corrected outright.
- **"Closing the 49-against-95 gap means writing 46 forms" is superseded.** The
  gap was 49 forms against 83 published statement formats — 95 was the diagram
  count, which includes 12 phrase fragments. The forms were written as review
  work, exactly as the ruling requires, and the catalog now carries 81 forms with
  12 recorded dispositions. The ruling that forms are authored rather than
  projected is unchanged; nothing was promoted mechanically.

## Not yet covered

| Subsystem | State |
|---|---|
| Db2 | 832 topics pinned and all 174 rows resolve. No syntax reader; statement syntax is published as the same DITA railroad markup COBOL uses, so the COBOL reader is the nearest starting point. |
| z/OSMF | REST families rather than a command language. All 216 rows resolve; a syntax projection does not apply without a different comparison model. |
| CICS, IMS, MQ | All three pin topics and all three reproduce. Their rows are located by `html-table:` and `html-link:` rather than by topic, so the locator audit reports them as skipped with that reason. None of the three has a syntax reader. CICS is the largest of them and the one most often assumed to be done: its 571 rows are `html-table:` locators into a single pinned topic, the EIBFN function-code table in `dfha8mf.html`, which gives an inventory anchored to the publication and nothing that reads the publication's syntax — the same position JCL and RACF were in before this work. There is no `conformance/0.9` directory and no CICS reader anywhere in the tree; 0.9.0, which owns the CICS API, is a proposed coverage version with no code behind it yet. |

The open items, in order of size:

1. **429 RACF operand names the publication publishes and the catalog does not
   carry**, and 62 it carries that the publication does not publish. The second
   set is dispositioned and gated; the first is reported and gated by nothing.
2. **688 AMS parameters against a grammar that records none**, pending the
   contract decision above.
3. **69 COBOL keywords across 29 rows** that appear in no catalog form.
4. **No syntax reader for Db2, CICS, IMS or MQ**, which between them carry 796
   of the 1,506 catalog rows (174 + 571 + 25 + 26); z/OSMF's 216 need a
   different comparison model rather than a reader.

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
  865 rows rather than 845, because the 20 JCL table rows it used to report as
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
  located". The current figures are 34/34 located, 751 source operands, 844
  values, 957 members, 62 catalog-only, 429 source-only, and they are read from
  `conformance/0.5/generated/racf-html-syntax-projection.json`. The nine
  "unlocated" commands were a property of the PDF reader.
- **COBOL numbers superseded.** Stop quoting "96 diagrams", "18 rows carry more
  source formats", "139 distinct keywords" and "Phrase fragments 13". The
  publication publishes 95 diagrams = 83 formats + 12 fragments; 2 rows carry
  more source formats than the catalog has forms; 69 distinct keywords across 29
  rows are missing. The form gap the section described is largely closed —
  catalog forms went 49 → 81 in commit 6223be2 — and
  `distinct_catalog_placeholders` rising 33 → 73 is a consequence of adopting the
  reference's own lowercase operand names, not a regression;
  `distinct_undefined_placeholders` (6, added in 2112209) is the metric that
  answers the question.
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
  **Three of its four resolutions fail a run, not one.**
  `UNEXPLAINED = (REPUBLISHED, SAME_DATE, UNDATED)` at
  `conformance/tools/fetch_pinned_sources.py:76` is what `topics_unexplained`
  counts and what decides `differs`: `republished`, `same-date-different-bytes`
  and `undated-difference` each fail; only `stale-read` does not. The
  db2-staleness finding recommended failing on `republished` alone, and the code
  went further on purpose — a same date over different bytes is the case neither
  story explains and the one a reviewer most needs to see, so saying it is
  benign is the opposite of what it is. A `differs` also stands when the manifest
  digest moves with no topic reporting a mismatch. Nothing is re-pinned.
- **What did not move.** No coverage claim. Every projection named here carries
  `coverage_credit: 0`; `conformance/0.2/evidence/coverage-ledger.json` still
  reads `official_compatibility_numerator: 0` and `generated_catalog_credit: 0`;
  the 1,506 row identities and `GENERATED_IDENTITY_SET_SHA256` are unchanged.
  Three waves of work deliberately moved no coverage number, and this record must
  not be read as though they did.
