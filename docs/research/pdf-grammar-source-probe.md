# Publication source probe

Status: **Diagnostic probes; zero coverage credit; not catalog authorities**

Each subsystem catalog cites an IBM publication, and nothing in the repository
checks a catalog against the publication it names. These probes build an
independent machine projection from the published source and diff it against
the committed catalog.

Every probe now runs on the pinned edition. `www.ibm.com/docs` returns 403 to
scripted HTTP clients, so `conformance/tools/browser_fetch.py` attaches to a
Chrome listening on a debugging port and issues same-origin `fetch` calls from
inside the page. `conformance/tools/fetch_pinned_sources.py` drives that over
the baseline index and verifies each download against the digest the catalog
was extracted from.

IBM publication bytes are not retained in the repository.

## The pins are reproducible

`conformance/0.2/catalogs/index.json` records a URL and a sha256 for every
baseline. Re-fetching all six PDF sources returns those exact bytes:

| Baseline | Publication | Bytes | Pin |
|---|---|---|---|
| cobol | SC27-8713-04, Enterprise COBOL 6.5 Language Reference, 916 pages | 4,385,427 | matches |
| jcl-jes2 | SA23-1385-70, z/OS 3.2 MVS JCL Reference, 758 pages | 3,652,434 | matches |
| dataset-vsam-ams | SC23-6846-70, z/OS 3.2 DFSMS Access Method Services, 610 pages | 2,857,720 | matches |
| racf-saf | SA23-2292-70, z/OS 3.2 RACF Command Language Reference, 746 pages | 3,735,292 | matches |
| zosmf | SC27-8430-70, z/OSMF Programming Guide, 1478 pages | 7,025,714 | matches |
| db2 | Db2 13 for z/OS SQL Reference, 3186 pages | 14,056,820 | **differs** (pinned 14,051,668) |

Five of six are byte-identical to their pin, so the version skew the earlier
probes carried is gone. Db2 differs because the SQL Reference is republished at
a stable URL — the baseline itself records "last updated 2026-08-13" — which is
a re-pin decision for a reviewer, not something a probe should do silently.

The four HTML baselines are not reproducible this way. Their pinned digests are
much smaller than anything the site serves (`mq.html` is pinned at 8,698 bytes),
and neither the served page (77,862 bytes of application shell) nor the rendered
DOM (204,570 bytes) hashes to them. The topic content itself comes from a
separate endpoint — `www.ibm.com/docs/api/v1/content/<product>%2F<topic>.html
?parsebody=true&lang=en` — which returns 10,245 bytes for that same MQ topic.
That is the right shape for the pin but not the same bytes, so the HTML pins
need a recorded retrieval method before they can be re-verified.

## Row identity checks out everywhere

Every PDF-backed catalog row carries a `pdf-page:N;outline:TITLE` locator.
`conformance/tools/verify_outline_locators.py` resolves each one against the
pinned publication's outline. This audits row *identity* rather than row
*content*, but it is uniform, and it covers the two baselines that have no
syntax reader of their own.

**845 of 845 outline-located rows resolve exactly** — the heading exists, at the
recorded page, in the pinned edition:

| Baseline | Rows | Exact | Not an outline locator |
|---|---|---|---|
| cobol | 173 | 173 | 0 |
| jcl-jes2 | 237 | 217 | 20 (`pdf-page:47;table:1`) |
| dataset-vsam-ams | 36 | 31 | 5 (`roadmap-normalization:`) |
| racf-saf | 48 | 34 | 14 (`html-table:`) |
| zosmf | 216 | 216 | 0 |
| db2 | 174 | 174 | 0 |

No heading was missing and none had moved. Whatever else is thin about these
catalogs, their inventories are anchored to the publications they cite.

## Each publication needs its own reader

The pinned PDF is the artifact of record: it is what the digests pin and what
the `pdf-page;outline` locators point into, so pin verification and the locator
audit read it and nothing else.

Syntax is a different job. Where a publication also has web topics, they are
the better source, because IBM generates them from the same DITA the PDF is
typeset from — the structure is stated in markup instead of being inferred from
where ink landed on a page. COBOL and AMS are read that way. Both readers
replaced a PDF reader that inferred structure from layout, and both corrections
went the same direction: the PDF reader had been merging or splitting levels
that the markup states outright.

The remaining two readers stay on the PDF because they need no such inference:
RACF's syntax is plain bracket notation in the text, and JCL's inventory is the
outline itself. RACF is the one that would likely still gain from the topics.

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
  (`[ AT([node].userid ...) | ONLYAT(...)]`). The reader locates the block by
  the reference's own introduction line, tolerates kerning that splits command
  names (`RV ARY`), and splits top-level operands from segment members by line
  shape, because the book contains unbalanced syntax lines that make a running
  parenthesis counter diverge.
- **AMS** puts its parameters in definition lists. A parameter is a top-level
  `dt`; the values it accepts are `dt` entries of a `dl` nested inside its own
  `dd`. Terms carry their argument and their alternation together
  (`INFILE(ddname)|INDATASET(entryname)`), so the reader removes arguments
  before splitting alternations — the other order stops at the first
  parenthesis and loses the second name.
- **JCL** publishes its inventory in the outline itself: one chapter per
  statement, one entry per parameter. The reader needs no syntax parsing.

## Results

### JCL — 204 statement parameters, all confirmed

| Unit | catalog | source | shared |
|---|---|---|---|
| dd-parameters | 74 | 74 | 74 |
| exec-parameters | 19 | 19 | 19 |
| job-parameters | 35 | 35 | 35 |
| output-parameters | 76 | 76 | 76 |

On the pinned 3.2 edition the match is exact in every unit. The nine
differences the earlier V2R2 run reported — `DSKEYLBL`, `NULLOVRD`, `ROACCESS`,
`ABDISPCC`, `TVSAMCOM`, `TVSMSG`, `EMAIL`, `GDGBIAS`, and the apostrophe in
`PROGRAMMER'S NAME` — were all edition skew. The JCL catalog is fully confirmed
by its publication.

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

### RACF — 34 command families

25 of 34 commands are now located, up from 23, after two reader fixes: a
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

- COBOL: the topics are the same product version as the pinned PDF but are not
  the pinned artifact, so the projection is checked against a source the
  baseline does not pin. `conformance/0.3/generated/cobol-topic-manifest.json`
  records each topic's path and digest so a reviewer can see exactly what was
  read. Inside an optional segment every branch is reported optional rather
  than alternative, because none of them can be required; alternation is
  therefore only visible on the main line. `EXIT` yields formats 1, 2, 3, 5 and
  6, so a format the reference documents without a diagram is invisible here.
- RACF: nine commands are not located. `RACDCERT`, `RACMAP`, `RACPRIV` and
  `RACPRMCK` publish one syntax block per function (`RACDCERT ALTMAP(...)`)
  rather than one per command, and `DISPLAY`, `RESTART`, `SIGNOFF`, `STOP` and
  `TARGET` are operator commands whose blocks the opener does not reach.
  Segment nesting is recovered by line shape, so a segment opened inline rather
  than on its own line leaks its members to top level. Unanchored matches are
  flagged rather than dropped.
- AMS: like COBOL, the topics are the same version as the pinned PDF but are
  not the pinned artifact; `conformance/0.6/generated/ams-topic-manifest.json`
  records each topic's path and digest. A parameter nested under another
  (`DEFINE PATH` documents `NAME` and `PATHENTRY` inside `PATH(...)`) is
  reported as a value, which is faithful to the reference's own nesting but
  means the parameter count is per level rather than per command.
- JCL: parameters documented outside a `... parameter` outline entry are not
  seen.
- Locator audit: an outline title that appears more than once matches any of
  its pages, so a row pointing at the wrong occurrence still reads as exact.

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
| Db2 | Retrieved and its 174 rows resolve against the outline, but the SQL Reference has been republished since the pin. No syntax reader; statement syntax is drawn as railroad diagrams like COBOL's. |
| z/OSMF | REST families rather than a command language. Its 216 rows resolve against the outline; a syntax projection does not apply without a different comparison model. |
| CICS, IMS, MQ | HTML baselines. The 0.9 CICS DITA reader already covers CICS; IMS and MQ pin small HTML snapshots whose retrieval method is not recorded, so their digests cannot be re-verified yet. |
