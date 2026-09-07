# COBOL grammar source probe

Status: **Diagnostic probe; zero coverage credit; not a catalog authority**

`conformance/0.3/cobol/language.json` records 173 reviewed row identities and
103 hand-normalized syntax sketches. Nothing in the repository checks those
sketches against the publication they cite. This probe builds an independent
machine projection of the same syntax and reports the difference.

## Source

The 0.9 CICS pipeline reads IBM Documentation HTML, where each command topic
ships a DITA syntax diagram whose CSS classes already type every keyword,
operand, and alternation. The COBOL Language Reference has no equivalent: its
web topics render syntax as images, and its published grammar exists only as
railroad diagrams drawn in the PDF.

This probe therefore reads the PDF. IBM publication bytes are not retained in
the repository.

| | |
|---|---|
| Publication | Enterprise COBOL for z/OS 6.4 Language Reference |
| File | `igy6lr40.pdf`, 906 pages, 4,345,979 bytes |
| Digest | `sha256:eef69c81ab8bcd569eaa2a47f430ff70c5518d4929cff26e7ab1ed6170f5c0bc` |
| Catalog compared | `conformance/0.3/cobol/language.json` (6.5, `SC27-8713-04`) |

The one-minor version skew is a known limitation: a difference this probe
reports may be a 6.4/6.5 delta rather than a catalog omission. Every row keeps
its evidence so a reviewer can tell the two apart.

## Method

`extract_cobol_pdf_grammar.py` replays each page content stream rather than
using flat text extraction, because the reference draws diagrams as inline
vector art whose labels carry no usable coordinates otherwise. It recovers:

- device coordinates for every text run, through the CTM and text matrices;
- real characters, through each subset font's `/ToUnicode` CMap;
- keyword versus operand, from the font style IBM uses for each;
- stroked rails, which separate diagrams from the 8pt code samples that share
  their type size;
- wrapped rail lines, alternatives stacked under a main-line item, and
  bypassed optional items.

`compare_cobol_grammar.py` then diffs the projection against the catalog rows.
Neither tool proposes catalog rows, and neither grants coverage credit.

## Result over the 44 procedure statements

| | catalog | source |
|---|---|---|
| Forms / diagrams | 49 | 95 |
| Named formats | — | 59 |
| Distinct operands named | 33 undefined placeholders | 75 named operands |

- 20 of 44 rows have more source diagrams than catalog forms.
- 37 of 44 rows use a keyword that appears in the diagram and in no catalog
  form; 137 distinct keywords are missing this way.
- 7 rows are keyword-complete: `CANCEL`, `CONTINUE`, `EVALUATE`, `GOBACK`,
  `RELEASE`, `STOP`, `UNSTRING`.

The gap is concentrated where the sketch collapses a format family. `SET` has
one catalog form against 16 named source formats; `INSPECT` has one against
four; `DIVIDE` one against five; `PERFORM` two against four. `JSON GENERATE`
and `XML GENERATE` each hide about 30 keywords behind a single line.

## Known limitations

- `CONTINUE` and other single-keyword diagrams are dropped by the two-token
  guard that rejects figure callouts.
- Alternation is inferred from horizontal overlap with the main line, so a
  stacked group whose main line is a bare rail reports its members as optional.
- Fragment references resolve to `PHRASE n` markers and are not inlined.
- The projection is a review input. Promoting any of it into the catalog
  requires the reviewed 6.5 publication and a separate decision per row.
