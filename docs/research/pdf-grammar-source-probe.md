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

Syntax is where the books diverge, and one extractor does not carry over:

- **COBOL** draws railroad diagrams as inline vector art. The reader replays
  the page content stream for coordinates, decodes subset fonts through their
  `/ToUnicode` CMaps, separates keywords from operands by font style, and uses
  stroked rails to tell diagrams from equally sized code samples.

  The PDF is not the only option. The 6.5 web topic, fetched through the
  browser, carries the same DITA markup CICS uses — `class="syntaxdiagram"`,
  `boxed syntaxkwd`, `boxed syntaxvar`, `groupchoice`, `groupseq` — so the
  structure is available without any geometry work. The `c.gif` image on that
  page is a fallback beside the SVG, not the diagram itself. The CICS reader
  still does not transfer unchanged: it requires every operand to appear as
  `OPTION(kind)` and rejects the bare `syntaxvar` operands COBOL uses.
- **RACF** has no diagrams. Each command carries a bracket-notation block
  (`[ AT([node].userid ...) | ONLYAT(...)]`). The reader locates the block by
  the reference's own introduction line, tolerates kerning that splits command
  names (`RV ARY`), and splits top-level operands from segment members by line
  shape, because the book contains unbalanced syntax lines that make a running
  parenthesis counter diverge.
- **AMS** documents parameters as flush-left headings under `Required
  Parameters` / `Optional Parameters`, with indented subparameters and prose
  beneath them, so column position does the parsing. `Abbreviation:` lines are
  collected alongside.
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

### AMS — 889 parameters against a catalog that records none

All 31 functional commands are located. The publication documents **889
parameters and 512 abbreviations**; `conformance/0.6/ams/grammar.json` records
`keywords: ["ALLOCATE"]` and nothing else, so its parameter inventory is
**zero for every command**.

The spread is wide: `ALLOCATE` 80, `ALTER` 75, `DEFINE CLUSTER` 64, `DCOLLECT`
60, `REPRO` 54, down to `VERIFY` 2. Abbreviations are a second surface the
catalog does not carry at all — `DELETE` alone documents `AIX`, `CL`, `GDG`,
`LIBENT`, `NVSAM`, `PGSPC`, `TNAME`, `UCAT`, `VOLENTRY`, `VOLENT`.

This is the largest gap any probe has found, and unlike the others it needs no
interpretation: the field is empty.

### COBOL — 44 procedure statements

| | catalog | source |
|---|---|---|
| Forms / diagrams | 49 | 95 |
| Named formats | — | 59 |
| Operand naming | 33 undefined placeholders | 75 named operands |

Moving from the 6.4 edition to the pinned 6.5 changed almost nothing (137
missing keywords became 136), which settles the question the earlier run left
open: this gap is not edition skew. 20 rows have more source diagrams than
catalog forms; 36 rows use a keyword that appears in no catalog form. Only
`CANCEL`, `CONTINUE`, `EVALUATE`, `GOBACK`, `RELEASE`, `STOP` and `UNSTRING`
are keyword-complete. The gap concentrates where the sketch collapses a format
family: `SET` has one form against 16 named source formats, `INSPECT` one
against four, `DIVIDE` one against five.

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

- COBOL: single-keyword diagrams such as `CONTINUE` are dropped by the
  two-token guard that rejects figure callouts; alternation is inferred from
  horizontal overlap, so a stacked group whose main line is a bare rail reports
  its members as optional; fragments resolve to `PHRASE n` markers.
- RACF: nine commands are not located. `RACDCERT`, `RACMAP`, `RACPRIV` and
  `RACPRMCK` publish one syntax block per function (`RACDCERT ALTMAP(...)`)
  rather than one per command, and `DISPLAY`, `RESTART`, `SIGNOFF`, `STOP` and
  `TARGET` are operator commands whose blocks the opener does not reach.
  Segment nesting is recovered by line shape, so a segment opened inline rather
  than on its own line leaks its members to top level. Unanchored matches are
  flagged rather than dropped.
- AMS: the reader takes flush-left headings, so a parameter the typesetter
  indented is missed and a value the typesetter did not indent is counted.
  Single-character names are excluded because the reference sets value letters
  (`AVGREC(U|K|M)`) on their own lines.
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
- **AMS parameters — not promoted.** The projection is review input. It reads
  flush-left headings, so it also collects values that the typesetter did not
  indent: `BLDINDEX` reports `ALL`, `CRITICAL` and `NONE`, which are values of
  `SORTMESSAGELEVEL`, not parameters. Writing 889 machine-read names into a
  contract artifact that drives the AMS parser would inject those errors into
  the emulator. The publication has no cleaner machine source — Chapter 3 is a
  prose summary table, and only `ALLOCATE` carries a bracket-notation syntax
  table.
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
