# ADR-0052: Separate COBOL data and intermediate precision

Status: Accepted for the bounded #158/#159 repair with documented source assumptions
Owner: COBOL compiler and decimal runtime maintainers
Scope: Typed ADD/COMPUTE plans and compile-time numeric admission
Applies from: mainframe-env newly compiled COBOL decimal plans

## Decision

This supersedes the COBOL arithmetic-policy selection in
[ADR-0011](0011-typed-language-hir-and-semantic-ir.md), retaining its storage,
receiver-update and condition contracts. COMPAT admits at most 18 data/literal
digits and carries 30 significant intermediate digits; EXTEND admits 31 and
carries 31. Data declarations and active, expanded numeric literals exceeding
the selected limit produce a compile-time diagnostic, even when unused.

Within the intermediate cap, finite addition, subtraction and multiplication
retain exact values. Division uses the complete significant-digit budget,
including for small quotients; leading fractional zeros do not consume that
budget. Nonterminating quotients and excess fractional intermediate digits
truncate toward zero. An integer part exceeding the cap raises SIZE ERROR:
there are no fractional digits that can be discarded to repair it. This is
an explicit bounded overflow assumption, not a claim of licensed equivalence.
Receiver scale, ROUNDED and SIZE ERROR conversion apply after expression
evaluation, independently for each captured receiver.

## Executable compatibility

The `mainframe-env.decimal-assignment-plan@2` header and expression encoding
remain unchanged. Append arithmetic-context tags 3 (`CobolCompatV2`) and 4
(`CobolExtendV2`). The compiler emits the selected tag; cap and division policy
are therefore deterministic from the plan without source parsing or module
configuration. Older readers reject the new tags as unsupported policies.

Tags 1 (`Decimal18V1`) and 2 (`Decimal34V1`) retain their historical 18/34-digit
contexts and nine division guard places, including the ledger adapter. The
legacy `@1` reader still obtains its context from module configuration and
preserves whole-batch atomicity. Existing bytes decode with their original
policies; no bytes are reinterpreted as the new COBOL profiles.

## Source review and evidence

Offline review used IBM Enterprise COBOL for z/OS 6.5 baseline
`ibm-enterprise-cobol-6.5-2026-05-31`, Language Reference topics
`SS6SG3_6.5/lr/ref/rlpdsxar.html` (arithmetic expressions) and
`SS6SG3_6.5/lr/ref/rlpdsaso.html` (arithmetic operands). The latter distinguishes
operand limits from the wider composite limits and describes multiple results.

The Programming Guide intermediate-results topics `cpinr02`, `rpinr04` and
ARITH topic `up0012` were unavailable in the offline cache. Refresh was not
authorized. The 30/31 digit caps come from public IBM documentation and the
orchestrator's supplied semantic decision, with independent GnuCOBOL
cross-checks rather than a newly pinned Programming Guide source corpus.

GnuCOBOL 3.2.0 (`/usr/bin/cobc -x -std=ibm`, scratch in `/tmp`) agrees on:
18-digit cancellation, twelve-place 1/3, nested (1/3)*2, small nested
(1/10^12)*10^12, 1/(3*10^11), negative 1/3, ROUNDED 2/3, rounding-induced
SIZE ERROR preserving the receiver, different-scale multiple receivers, and
20-digit EXTEND addition into a 21-digit receiver. These cases do not establish
GnuCOBOL support for IBM's profile-specific declaration limits or prove all
cap-boundary behavior. Unit regressions separately pin 30/31-digit fractional
truncation and integer overflow. No licensed execution credit is claimed.
