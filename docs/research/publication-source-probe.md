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

IBM publication bytes are not retained in the repository.

## The pins are reproducible

`conformance/0.2/catalogs/index.json` records, per baseline, the
table-of-contents URL and its sha256, the content URL template
(`?parsebody=true&lang=en` is part of the pin — dropping it moves the MQ topic
from 10,245 to 8,264 bytes), and a manifest under `conformance/0.2/manifests/`
that names every topic the baseline was read from with that topic's own digest.
The baseline's `sha256` is a digest over that ordered topic list, not over a
downloaded file.

`conformance/tools/fetch_pinned_sources.py` re-reads all of it: the table of
contents, then every topic the manifest names, then recomputes the manifest
digest from the bytes it got back. Before asking IBM anything it checks each
manifest against itself, because a manifest whose digest no longer follows from
its own topic list is a defect retrieval cannot detect. Re-reading returns those
exact bytes for eight of the nine books. The ninth, db2, does not re-read the
same way twice, which is why its row below says **differs** and why "all nine
reproduce" is not a sentence this record makes:

| Baseline | Publication | Topics | Bytes | Pin |
|---|---|---|---|---|
| cobol | SC27-8713-04, Enterprise COBOL 6.5 Language Reference | 622 | 8,760,265 | matches |
| cics | Function codes of EXEC CICS commands, CICS TS 6.x | 1 | 265,761 | matches |
| jcl-jes2 | SA23-1385-70, z/OS 3.2 MVS JCL Reference | 1,985 | 5,419,261 | matches |
| dataset-vsam-ams | SC23-6846-70, z/OS 3.2 DFSMS Access Method Services | 516 | 3,448,164 | matches |
| racf-saf | SA23-2292-70, z/OS 3.2 RACF Command Language Reference | 109 | 4,011,649 | matches |
| zosmf | SC27-8430-70, z/OSMF Programming Guide | 395 | 9,761,388 | matches |
| db2 | Db2 13 for z/OS SQL Reference | 832 | 29,147,264 | **differs** (see below) |
| ims | Comparing EXEC DLI commands and DL/I calls, IMS 15.6 | 1 | 16,046 | matches |
| mq | IBM MQ 9.4 MQI call descriptions | 27 | 837,828 | matches |

4,488 topics and 61,667,626 bytes. Eight of nine reproduce exactly — 0 changed,
0 unreachable — and all nine tables of contents hash to their recorded
`toc_sha256`. The RACROUTE router-interface topic that RACF pins as a supporting
source reproduces too, at 161,025 bytes; no tool in this repository had ever
re-read it, because the old loop read only each baseline's primary source.

The content endpoint is byte-stable and the rendered DOM is not. That is the
whole reason the pins moved. A rendered capture carries a fresh `lit$<random>$`
nonce and an Adobe `eto_<hex>` nonce on every load, so the three HTML baseline
digests this record used to describe as "not reproducible this way" — cics, ims
and mq, plus the RACROUTE supporting pin — were never reproducible by any
retrieval mode: they were captures of the DOM.

### Db2 does not re-read deterministically

Repeated full re-reads of the 832 Db2 topics do not agree with each other.
Three runs reported 1, 1 and 6 topics changed and did not name the same topics
twice; a fourth reported 5. What every run agrees on is the *direction* of the
difference: every served body was *smaller* than its pin and carried an
*earlier* Last Updated date — 2026-01-07, 2026-04-24, 2026-05-12, 2026-05-19
against a pinned 2026-09-03. Sequential re-fetches of a named topic are stable
in the moment, and it is the moment that changes: `db2z_sql_explain.html` was
reported changed by two full runs and then returned the pinned bytes 40/40 under
eight concurrent workers minutes later, while `db2z_sql_createview.html`
returned the older revision 30/30 in the same window.

The whole difference between the pinned revision and the older one is
typographic. Diffing the pinned `db2z_sql_createview.html` against the one
served now gives two lines: the Last Updated date, and `SQL statements in
Db2&nbsp;for&nbsp;z/OS` against `SQL statements in Db2 for z/OS` in a
cross-reference title. Two `&nbsp;` entities are exactly the 10 bytes the report
says are missing. The 2026-09-03 republication of these topics inserted
non-breaking spaces; no normative content moved.

What the cause is remains unresolved. The pattern rules out the reading that
would matter most — a book IBM had edited since the pin would not serve its
newer revision to one run and its older one to the next, and its dates would
move forwards rather than backwards — so this is not publication drift and
nothing is re-pinned on the strength of it. Beyond that, nothing here identifies
which component serves the older revision or when it will stop, and no
re-verification run so far has been reproducible enough to say. The claim this
record makes is therefore the narrow one: **one baseline of nine does not
re-read deterministically, and the shape of the non-determinism is the one
above.** Neither "the Db2 pin has drifted" nor "all nine reproduce" is
supported.

That makes it a review decision rather than a failure, which is the case the
policy was written for. The report records each changed topic's served Last
Updated date beside the pinned one, so "IBM edited a paragraph" stays separable
from "something served us an older revision" from "the endpoint changed what it
emits" from "we changed what we ask for". A re-verification of this book will
sometimes go red for reasons no one in this repository controls, and the way to
read that red is to check whether the served dates went backwards before
treating it as drift.

An unreachable endpoint reports `skipped` and a partial run reports `sampled`;
neither can be mistaken for a match.

## Row identity checks out everywhere

A topic-located catalog row carries a `topic:PATH;topic-id:SLUG;heading:TITLE`
locator, and `conformance/tools/verify_topic_locators.py` resolves each one
against the live publication, checking three things: the topic path answers, the
tree publishes that topic-id slug at that path, and the heading still reads what
the row says it reads. This audits row *identity* rather than row *content*, but
it is uniform, and it is the only check the four baselines with no syntax reader
of their own — db2, zosmf, and now jcl-jes2 and racf-saf — get at all.

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
reason rather than dropped, so 865 + 641 accounts for all 1,506 rows.

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
nothing else. Those labels are frozen row identity and cannot be rewritten, so
the comparison gives way instead. The 20 JCL statement rows cite a topic and one
row of a shared table together, and are checked against that table's cells; a
topic that no longer carries the named table reports `retitled`, not a silent
pass.

Whatever else is thin about these catalogs, their inventories are anchored to
the publications they cite.

## Each publication needs its own reader

The web topics are the artifact of record for every job here. They are what the
digests pin, what the `topic:PATH;topic-id:SLUG;heading:TITLE` locators point
into, and what every reader reads. IBM generates them from the same DITA the
printed book is typeset from, so structure is stated in markup instead of being
inferred from where ink landed on a page.

That is not a preference; it is the finding. Both readers that were replaced
went the same direction, and both corrections were of the same kind: the PDF
reader had been merging or splitting levels the markup states outright.
Replacing the COBOL reader turned 95 diagrams and 59 titles into 83 statement
formats and 13 phrase fragments and disagreed per row in 16 of 44 statements.
Replacing the AMS reader turned 889 flat names into 688 parameters and 193
values. Neither publication changed; only the source we read it from did.

COBOL and AMS have topic readers. RACF and JCL do not yet — their PDF readers
are deleted with no replacement in this wave, so the RACF and JCL results below
are marked provisional. RACF is the one that will gain most from the topics: its
PDF reader had to tolerate kerning that split command names and had to recover
segment nesting from line shape, and both of those are stated outright in the
markup.

With that split, one extractor still does not carry over:

- **COBOL** publishes its railroad diagrams twice. The PDF draws them as vector
  art, and the web topics carry the DITA markup CICS uses. The reader takes the
  markup, because the structure is stated there rather than inferred:

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

  The CICS reader does not transfer unchanged: it requires every operand to
  appear as `OPTION(kind)` and rejects the bare `syntaxvar` operands COBOL uses.
- **RACF** has no diagrams. Each command carries a bracket-notation block
  (`[ AT([node].userid ...) | ONLYAT(...)]`). The deleted PDF reader located the
  block by the reference's own introduction line, tolerated kerning that split
  command names (`RV ARY`), and split top-level operands from segment members by
  line shape, because the typeset book contains unbalanced syntax lines that make
  a running parenthesis counter diverge. Every one of those three is a
  compensation for reading ink; the topics state the block, the command name and
  the segment nesting outright, so none of them should survive into the
  replacement.
- **AMS** puts its parameters in definition lists. A parameter is a top-level
  `dt`; the values it accepts are `dt` entries of a `dl` nested inside its own
  `dd`. Terms carry their argument and their alternation together
  (`INFILE(ddname)|INDATASET(entryname)`), so the reader removes arguments
  before splitting alternations — the other order stops at the first
  parenthesis and loses the second name.
- **JCL** publishes its inventory in the book structure itself: one chapter per
  statement, one entry per parameter. The reader needs no syntax parsing, and
  the deleted PDF reader walked the PDF outline. Its replacement should walk the
  topic tree, which the pin already records and which the locator audit already
  resolves all 237 JCL rows against.

## Results

### JCL — 204 statement parameters, all confirmed (provisional)

**This result is PDF-derived and cannot currently be re-derived.** It was
produced by `conformance/0.7/tools/extract_jcl_pdf_parameters.py`, which read
the PDF outline of the pinned edition and is deleted. Nothing in the tree
reproduces the table below until a topic reader lands. Read it as a finding from
a source we have stopped reading, not as a claim this repository can stand
behind today. It was never gated, and nothing downstream depends on it: the
comparison confirmed a catalog it did not change.

| Unit | catalog | source | shared |
|---|---|---|---|
| dd-parameters | 74 | 74 | 74 |
| exec-parameters | 19 | 19 | 19 |
| job-parameters | 35 | 35 | 35 |
| output-parameters | 76 | 76 | 76 |

On the 3.2 edition the match was exact in every unit. The nine differences the
earlier V2R2 run reported — `DSKEYLBL`, `NULLOVRD`, `ROACCESS`, `ABDISPCC`,
`TVSAMCOM`, `TVSMSG`, `EMAIL`, `GDGBIAS`, and the apostrophe in `PROGRAMMER'S
NAME` — were all edition skew. What still stands independently of the deleted
reader is that all 237 JCL rows resolve against the pinned topics; what needs a
topic reader before it can be restated is the parameter comparison above.

### AMS — 688 parameters against a catalog that records none

All 31 functional commands are located. The publication documents **688
parameters**, plus **193 values** those parameters accept;
`conformance/0.6/ams/grammar.json` records `keywords: ["ALLOCATE"]` and nothing
else, so its parameter inventory is **zero for every command**. The spread is
wide: `ALLOCATE` 56, `DELETE` 30, `BLDINDEX` 14, down to `VERIFY` 2.

**These numbers correct an earlier PDF-derived run**, which reported 889
parameters. That reader took flush-left headings, and the reference sets values
flush left too, so it counted `SORTMESSAGELEVEL`'s `ALL`, `CRITICAL` and `NONE`
as parameters of `BLDINDEX` — 25 names where the command has 14. The totals
were again close (889 against 688 + 193 = 881) because the PDF reader was
merging two levels rather than inventing names, but the split it produced was
wrong.

This remains the largest gap any probe has found, and it needs no
interpretation: the field is empty.

### COBOL — 44 procedure statements

| | catalog | source |
|---|---|---|
| Forms / diagrams | 49 | 96 |
| Statement formats | — | 83 |
| Phrase fragments | — | 13 |
| Operand naming | 33 undefined placeholders | 80 named operands |

Every one of the 96 diagrams is titled, and 18 rows carry more source formats
than the catalog has forms. 37 rows use a keyword that appears in no catalog
form, 139 distinct keywords in total. The gap concentrates where the sketch
collapses a format family: `JSON PARSE` has one form against seven source
diagrams, `SET` one against eight, `INSPECT` one against four.

The counts are read against the phrase split. `JSON GENERATE` publishes one
statement format and five phrase diagrams (`when-phrase Format`,
`converting-phrase Format 1`, ...); counting those as formats would overstate
how many ways the statement can be written, so the projection marks each
diagram `format` or `fragment`.

**These numbers correct an earlier PDF-derived run**, which reported 95
diagrams against only 59 titles. The totals were close by coincidence: the
geometry reader over-split `INSPECT` into nine diagrams where the publication
has four and `SORT` into four where it has two, missed `START`, `UNSTRING` and
`GOBACK` entirely, and double-counted `SET` titles (16 against the true 7). Per
row the two disagreed in 16 of 44 cases. The finding itself survives — the
catalog forms substantially under-describe the publication — but the earlier
per-row figures should not be quoted.

### RACF — 34 command families (provisional)

**This result is PDF-derived and cannot currently be re-derived.** It was
produced by `conformance/0.5/tools/extract_racf_pdf_syntax.py`, which read the
bracket-notation blocks out of the pinned PDF's text and is deleted. The operand
counts below, and the nine unlocated commands, are properties of that reader as
much as of the publication, and a topic reader is expected to change both. One
thing it motivated does stand on its own and is already committed: the alias fix
below rests on the abbreviations RACF operators type, cross-checked against the
reference, not on the reader's output.

25 of 34 commands were located, up from 23, after two reader fixes: a
footnote carried across a page break was truncating blocks at the page
boundary, and commands with no alias (`[subsystem-prefix]RACLINK`) open their
block without the brace group the opener required.

| | |
|---|---|
| Catalog operands (located rows) | 333 |
| Source operands | 534 |
| Shared | 244 |
| Only in catalog | 89 |

The alias finding was firm — every located command carried a source alias
(`AD`, `AU`, `ALU`, `PE`, `RDEF`) that the catalog did not record — and it is
now **fixed**: 22 families carry their documented abbreviation, and `PASSWORD`
gained `PW` beside the `PHRASE` it already had. `SET` was excluded because its
match was unanchored and returned operand values (`SETONLY`, `NOSET`) rather
than an alias; `RACLINK` and `RVARY` genuinely have none. Five catalog rows
still carry no operands at all.

The operand counts are better than the first run — only-in-catalog fell from
142 to 89 — but they are still a reader-limited comparison rather than a clean
audit, because nine commands remain unlocated and some blocks may still close
early.

## Known limitations

- COBOL: `conformance/0.3/generated/cobol-topic-manifest.json` records each
  topic's path and digest so a reviewer can see exactly what was read. All 139
  of them are inside the 622 topics `conformance/0.2/manifests/cobol-topics.json`
  pins, at identical digests, so the projection is now read from the pinned
  artifact rather than from a same-version stand-in. Inside an optional segment
  every branch is reported optional rather than alternative, because none of
  them can be required; alternation is therefore only visible on the main line.
  `EXIT` yields formats 1, 2, 3, 5 and 6, so a format the reference documents
  without a diagram is invisible here.
- RACF (limitations of the deleted PDF reader, recorded so its replacement can
  be checked against them): nine commands were not located. `RACDCERT`,
  `RACMAP`, `RACPRIV` and `RACPRMCK` publish one syntax block per function
  (`RACDCERT ALTMAP(...)`) rather than one per command, and `DISPLAY`,
  `RESTART`, `SIGNOFF`, `STOP` and `TARGET` are operator commands whose blocks
  the opener did not reach. Segment nesting was recovered by line shape, so a
  segment opened inline rather than on its own line leaked its members to top
  level. Unanchored matches were flagged rather than dropped.
- AMS: like COBOL, all 89 topics in
  `conformance/0.6/generated/ams-topic-manifest.json` are inside the 516 that
  `conformance/0.2/manifests/dataset-vsam-ams-topics.json` pins, at identical
  digests. A parameter nested under another (`DEFINE PATH` documents `NAME` and
  `PATHENTRY` inside `PATH(...)`) is reported as a value, which is faithful to
  the reference's own nesting but means the parameter count is per level rather
  than per command.
- JCL (limitation of the deleted PDF reader): parameters documented outside a
  `... parameter` outline entry were not seen.
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

## Not yet covered

| Subsystem | State |
|---|---|
| Db2 | 832 topics pinned and all 174 rows resolve. No syntax reader; statement syntax is published as the same DITA railroad markup COBOL uses, so the COBOL reader is the nearest starting point. |
| z/OSMF | REST families rather than a command language. All 216 rows resolve; a syntax projection does not apply without a different comparison model. |
| JCL, RACF | Rows resolve — 237 and 34 — but both syntax readers were PDF readers and are deleted. Their results above are provisional until topic readers land. |
| CICS, IMS, MQ | All three pin topics and all three reproduce. Their rows are located by `html-table:` and `html-link:` rather than by topic, so the locator audit reports them as skipped with that reason. None of the three has a syntax reader. CICS is the largest of them and the one most often assumed to be done: its 571 rows are `html-table:` locators into a single pinned topic, the EIBFN function-code table in `dfha8mf.html`, which gives an inventory anchored to the publication and nothing that reads the publication's syntax — the same position JCL and RACF were in before this wave. There is no `conformance/0.9` directory and no CICS reader anywhere in the tree; 0.9.0, which owns the CICS API, is a proposed coverage version with no code behind it yet. |

## Corrections

This record is the probes' current state, not a dated snapshot, so it is
rewritten when what it describes changes. The log below says what changed and
when, so a reader who quoted an older revision can tell whether the number they
quoted still stands. Dated *delivery* records are the opposite — those are
corrected beneath the original sentence, never edited — and one such correction,
in `docs/delivery/coverage-versions/status/0.2.0.md`, covers the same migration.

**2026-09-07 — PDF is retired as a source, and this file was rewritten to say
so.** It was previously named `pdf-grammar-source-probe.md`. What changed, and
what a reader of the earlier revision should stop quoting:

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
- **The Db2 re-pin question changed shape and did not go away.** The old table
  reported db2 as the one baseline whose bytes differed from its pin, because
  the SQL Reference is republished at a stable URL and one whole-file digest
  flipped. Db2 is still the one baseline that does not reproduce, but for a
  different and smaller reason: the edge serves two revisions of a handful of
  topics, and a re-read lands on whichever node answers. That is diagnosed
  above, and it is a review decision either way. The `Db2 pin — not re-pinned`
  entry under "What a probe finding may and may not become" is kept as written.
- **The locator audit is a different tool, over more rows.**
  `conformance/tools/verify_outline_locators.py` is deleted;
  `conformance/tools/verify_topic_locators.py` resolves the
  `topic:PATH;topic-id:SLUG;heading:TITLE` locators the rows carry now. It covers
  865 rows rather than 845, because the 20 JCL table rows it used to report as
  "not an outline locator" now cite a topic and a table together. The old known
  limitation — that a repeated outline title matched any of its pages, so a row
  pointing at the wrong occurrence still read exact — is gone with the page
  numbers.
- **The JCL and RACF results are provisional until a topic reader replaces
  them.** `extract_jcl_pdf_parameters.py` and `extract_racf_pdf_syntax.py` are
  deleted with no replacement in this wave, so the JCL "204 statement
  parameters, all confirmed" table and the RACF "25 of 34 commands located, 534
  source operands" table can no longer be re-derived from anything in the tree.
  Neither was ever gated. Nothing downstream depends on them: the RACF alias fix
  they motivated is already committed and stands on the abbreviations RACF
  operators type, and the JCL comparison confirmed a catalog it did not change.
  Read both tables as findings from a source we have stopped reading, not as
  claims this repository can still stand behind. Both are marked provisional in
  place, above.
