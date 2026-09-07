# Publication source probe

Status: **Diagnostic probes; zero coverage credit; not catalog authorities**

Each subsystem catalog cites an IBM publication, and nothing in the repository
checks a catalog against the publication it names. These probes build an
independent machine projection from the published PDF and diff it against the
committed catalog.

`www.ibm.com/docs` returns 403 to this environment for every path under
`/docs`, including the URLs recorded in the 0.9 CICS manifest. The legacy
`publib*.boulder.ibm.com/epubs/pdf/` hosts are reachable and serve the same
publications at older editions, so every probe below carries a version skew
against its catalog. A reported difference may be an edition delta rather than
a catalog omission; each row keeps its evidence so a reviewer can tell them
apart. IBM publication bytes are not retained in the repository.

## Sources

| Subsystem | Publication | Digest | Catalog compared |
|---|---|---|---|
| COBOL | Enterprise COBOL for z/OS 6.4 Language Reference, `igy6lr40.pdf`, 906 pages | `sha256:eef69c81ab8bcd569eaa2a47f430ff70c5518d4929cff26e7ab1ed6170f5c0bc` | `conformance/0.3/cobol/language.json` (6.5) |
| RACF | z/OS V2R2 Security Server RACF Command Language Reference, `ich2a411.pdf`, 790 pages | `sha256:901e79febbac0625f48d6ba5ded4afe36e0864b805dab50995e1a3cd0d8cf3de` | `conformance/0.5/racf/command-language.json` (z/OS 3.2) |
| JCL | z/OS V2R2 MVS JCL Reference, `iea3b611.pdf`, 756 pages | `sha256:54c9a37d1a3a7cc3832cf95079e595ae532a12b98b60da5d38515846bb1908d3` | `conformance/0.2/catalogs/jcl-jes2.json` (z/OS 3.2) |

## Each publication needs its own reader

The three books present syntax in three different ways, so one extractor does
not carry over:

- **COBOL** draws railroad diagrams as inline vector art. The reader replays
  the page content stream for coordinates, decodes subset fonts through their
  `/ToUnicode` CMaps, separates keywords from operands by font style, and uses
  stroked rails to tell diagrams from equally sized code samples.
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

23 of 34 commands were located. Across those, the catalog records 319 operands
and the source 504, sharing 228: 276 appear only in the source and 91 only in
the catalog. Every located command carries a source alias (`AD`, `AU`, `ALU`,
`PE`, `RDEF`) that the catalog does not record — `aliases` is empty on all but
`PASSWORD`. Five catalog rows carry no operands at all.

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
