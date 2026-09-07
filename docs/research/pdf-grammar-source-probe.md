# Publication source probe

Status: **Diagnostic probes; zero coverage credit; not catalog authorities**

Each subsystem catalog cites an IBM publication, and nothing in the repository
checks a catalog against the publication it names. These probes build an
independent machine projection from the published PDF and diff it against the
committed catalog.

`www.ibm.com/docs` returns 403 to every scripted HTTP client here, including
the URLs recorded in the 0.9 CICS manifest, so the first probes read older
editions from the legacy `publib*.boulder.ibm.com/epubs/pdf/` hosts and carried
a version skew against their catalogs.

A real browser is not blocked. `conformance/tools/browser_fetch.py` attaches to
a Chrome listening on a debugging port and issues same-origin `fetch` calls
from inside the page, which reaches the current editions. The RACF reference
retrieved that way hashes to
`sha256:f4c8860aeb4d00b78f9257b28b2d880bd7571d74e2e00b2b1424b203801d5a46` —
byte-for-byte the digest already pinned in `conformance/0.2/tools/
extract_official_catalogs.py` and in `command-language.json`. The pinned
sources are therefore reproducible, and probes can drop the skew caveat as
each one is moved onto the pinned edition.

IBM publication bytes are not retained in the repository.

## Sources

| Subsystem | Publication | Digest | Catalog compared |
|---|---|---|---|
| COBOL | Enterprise COBOL for z/OS 6.4 Language Reference, `igy6lr40.pdf`, 906 pages | `sha256:eef69c81ab8bcd569eaa2a47f430ff70c5518d4929cff26e7ab1ed6170f5c0bc` | `conformance/0.3/cobol/language.json` (6.5) |
| RACF | z/OS 3.2 Security Server RACF Command Language Reference, `icha400_v3r2.pdf`, 746 pages — the pinned edition, fetched through Chrome | `sha256:f4c8860aeb4d00b78f9257b28b2d880bd7571d74e2e00b2b1424b203801d5a46` | `conformance/0.5/racf/command-language.json` (z/OS 3.2) |
| JCL | z/OS V2R2 MVS JCL Reference, `iea3b611.pdf`, 756 pages | `sha256:54c9a37d1a3a7cc3832cf95079e595ae532a12b98b60da5d38515846bb1908d3` | `conformance/0.2/catalogs/jcl-jes2.json` (z/OS 3.2) |

## Each publication needs its own reader

The three books present syntax in three different ways, so one extractor does
not carry over:

- **COBOL** draws railroad diagrams as inline vector art in the PDF. The reader
  replays the page content stream for coordinates, decodes subset fonts through
  their `/ToUnicode` CMaps, separates keywords from operands by font style, and
  uses stroked rails to tell diagrams from equally sized code samples.

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
- **JCL** publishes its inventory in the outline itself: one chapter per
  statement, one entry per parameter. The reader needs no syntax parsing.

## Results

### COBOL — 44 procedure statements

| | catalog | source |
|---|---|---|
| Forms / diagrams | 49 | 95 |
| Named formats | — | 59 |
| Operand naming | 33 undefined placeholders | 75 named operands |

20 rows have more source diagrams than catalog forms; 37 rows use a keyword
that appears in no catalog form, 137 distinct keywords in total. Only
`CANCEL`, `CONTINUE`, `EVALUATE`, `GOBACK`, `RELEASE`, `STOP` and `UNSTRING`
are keyword-complete. The gap concentrates where the sketch collapses a format
family: `SET` has one form against 16 named source formats, `INSPECT` one
against four, `DIVIDE` one against five.

### RACF — 34 command families

Against the pinned edition, 23 of 34 commands were located. The catalog records
319 operands and the projection 323, sharing 177.

One finding is already firm: **every located command carries a source alias**
(`AD`, `AU`, `ALU`, `PE`, `RDEF`) that the catalog does not record. `aliases`
is empty on every row except `PASSWORD`. Five catalog rows carry no operands at
all.

The operand counts are **not yet a clean audit**. The reader was tuned against
the V2R2 layout; on the pinned edition it still truncates some blocks, so part
of the 142 names reported as catalog-only (`ADDSD`'s `AUDIT`, `DATA`, `FROM`,
`GENERIC`) are present in the publication and missed by the reader. Those
counts need the reader retuned before any of them is read as a catalog gap.

### JCL — 204 statement parameters

| Unit | catalog | source | shared | only in catalog |
|---|---|---|---|---|
| dd-parameters | 74 | 71 | 71 | `DSKEYLBL`, `NULLOVRD`, `ROACCESS` |
| exec-parameters | 19 | 16 | 16 | `ABDISPCC`, `TVSAMCOM`, `TVSMSG` |
| job-parameters | 35 | 33 | 32 | `EMAIL`, `GDGBIAS`, `PROGRAMMER'S NAME` |
| output-parameters | 76 | 76 | 76 | — |

195 of 204 match exactly. Every remaining difference is explained by the
V2R2-to-3.2 edition gap except `PROGRAMMER'S NAME`, where the catalog uses a
straight apostrophe and the publication a typographic one. The JCL catalog is
the only one of the three that the publication substantially confirms.

## Known limitations

- COBOL: single-keyword diagrams such as `CONTINUE` are dropped by the
  two-token guard that rejects figure callouts; alternation is inferred from
  horizontal overlap, so a stacked group whose main line is a bare rail reports
  its members as optional; fragments resolve to `PHRASE n` markers.
- RACF: 11 commands are not located, mostly operator commands whose blocks the
  brace pattern does not reach; segment nesting is recovered by line shape, so
  a segment opened inline rather than on its own line leaks its members to top
  level. Unanchored matches are flagged rather than dropped.
- JCL: parameters documented outside a `... parameter` outline entry are not
  seen.

## Not yet covered

| Subsystem | State |
|---|---|
| AMS/VSAM | `dgt3i210.pdf` (V2R2, `sha256:2a4f659300852727466e4d4be03a9581eaee6b80c14c0d6939491832e4269f41`) retrieved; one chapter per command with a `<CMD> Parameters` section. Reader not written. `conformance/0.6/ams/grammar.json` records no parameters at all, so the whole inventory is currently unchecked. |
| DB2 | Publication not located on a reachable host. |
| z/OSMF | REST families rather than a command language; a syntax projection does not apply without a different comparison model. |
| IMS, MQ | The 0.2 baseline pins HTML sources, not PDFs, so the PDF readers do not apply. |
