# COBOL 0.4.0 Report Writer scope decision

Decision: Report Writer source-language items are outside the frozen Enterprise
COBOL 6.5 language-row denominator for 0.4.0. They require a separately
installed Report Writer Precompiler and therefore are not missing native
Enterprise COBOL statement or data-clause implementations.

## Authorities

- The pinned 0.3 language inventory is derived from Enterprise COBOL 6.5
  Language Reference publication `SC27-8713-04`, digest
  `sha256:8b86cbd2d838d8f460dcbe8d1e74d2266799ce1534f4cfdbc08469c36748e85e`.
  It contains no Report Writer `INITIATE`, `GENERATE`, or `TERMINATE` row,
  `REPORT SECTION`, or `USE BEFORE REPORTING` row. JSON and XML `GENERATE` are
  distinct pinned Enterprise COBOL statements.
- IBM's [Enterprise COBOL 6.5 Report Writer support
  documentation](https://www.ibm.com/docs/en/cobol-zos/6.5.0?topic=support-report-writer)
  says that the feature is supported through the Report Writer Precompiler and
  that version 1.6.01 or later is required for Enterprise COBOL 5.1 and later.
- IBM's [Enterprise COBOL 6.5 Migration
  Guide](https://www.ibm.com/docs/en/SS6SG3_6.5/pdf/mg.pdf) lists the Report
  Writer language items as accepted only when that precompiler is installed.

## Compatibility boundary

The 0.4 runtime can execute standard Enterprise COBOL emitted or converted by
an external Report Writer precompiler when that output stays within the pinned
language rows. Implementing, bundling, or claiming compatibility for the
precompiler itself is outside 0.4.0 and would require a separately pinned
product receipt, catalog, obligations, and licensed differential campaign.

This decision does not exclude Enterprise COBOL declaratives. The pinned `USE`
compiler-directing row remains structurally recognized and validated, while
the in-scope `USE AFTER STANDARD ERROR PROCEDURE` file interaction has explicit
execution and checkpoint/restart evidence through the `READ` row.

## Corrections

**2026-09-07 — both authorities this record cites are PDF references that no
longer resolve inside the repository.** The decision itself is unaffected: it
rests on Report Writer requiring a separately installed precompiler, and IBM
still states that. The original text is left standing as the dated record.

- The digest
  `sha256:8b86cbd2d838d8f460dcbe8d1e74d2266799ce1534f4cfdbc08469c36748e85e`
  under "Authorities" was the SC27-8713-04 PDF, pinned at
  `https://www.ibm.com/docs/en/SS6SG3_6.5/pdf/lrmvs.pdf`. That pin is retired.
  The COBOL baseline now pins 622 IBM Documentation topics listed in
  `conformance/0.2/manifests/cobol-topics.json`, digest
  `sha256:9b0291045414ca38d0e8138c7cc78d7321887a3fad6c27a433efd9e9ca7fd1ad`.
  The inventory it yields is unchanged, and it still contains no Report Writer
  `INITIATE`, `GENERATE` or `TERMINATE` row, `REPORT SECTION`, or
  `USE BEFORE REPORTING` row.
- The Migration Guide claim is published as a topic: [Report Writer language
  items
  affected](https://www.ibm.com/docs/en/cobol-zos/6.5.0?topic=writer-report-language-items-affected)
  (`SS6SG3_6.5/migrate/igymch1012.html`, Last Updated 2026-01-30) lists **ten**
  items "accepted by Enterprise COBOL only when the Report Writer precompiler is
  installed". The topic body carries exactly one `<ul>` and ten `<li>` entries,
  and the whole list is short enough to quote, so the count can be re-checked
  without a tool: `GENERATE statement`, `INITIATE statement`, `LINE-COUNTER
  special register`, `Alphanumeric literal IS mnemonic-name`, `PAGE-COUNTER
  special register`, `PRINT-SWITCH special register`, `REPORT clause of FD
  entry`, `REPORT SECTION`, `TERMINATE statement`, `USE BEFORE REPORTING
  declarative`. Every item this record's "Authorities" section relied on —
  `GENERATE`, `INITIATE`, `TERMINATE`, `REPORT SECTION` and `USE BEFORE
  REPORTING` — is among the ten. Cite that in place of `pdf/mg.pdf`.
