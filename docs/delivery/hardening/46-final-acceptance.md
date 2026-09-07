# Hardening review #46 — integrated acceptance

## Status

Pending one exact integrated full-assurance candidate. The eleven independently
reviewable findings tracked by #47 through #57 are implemented, merged, and closed,
but this document deliberately does not treat child-PR success as release-level
acceptance for #46.

The published `mainframe-env-v0.8.2` tag predates the final #57, #56 disposition,
and #54 assurance-tier merges, so that tag is not evidence that the complete #46
set passed together.

## Integrated closure rule

Close #46 only after a single candidate containing every #47-#57 disposition has
all selected permanent gates green and a full assurance campaign records that same
candidate. At minimum the receipt must include formatting, workspace tests, Clippy,
complete conformance, evidence seal, runtime architecture, the affected backend
parity tier, and the behavioral-mutation tier when selected. Skipped work is not a
pass.

Licensed IBM differential credit is unchanged by this hardening campaign. Local,
model, GnuCOBOL, synthetic, and reference evidence cannot be promoted to licensed
IBM equivalence.

## CI candidate handling

The selector computes the candidate from the event checkout. Downstream jobs now
checkout the same event ref normally and fail closed unless `git rev-parse HEAD`
exactly equals the selector candidate. This avoids requiring checkout to rediscover
a synthetic pull-request merge object by raw SHA while preserving exact-candidate
identity.

For build-selected runs, `v0-foundation` records the selector plan again inside the
exact-candidate assurance receipt, so the selector does not upload a duplicate plan
artifact. A selector-only plan is retained when no build tier is selected. Short
retention is intentional for these reproducible CI receipts; repository sources,
contracts, and tests remain the durable reproduction authority.

## Closure record

Do not mark this section accepted until the full campaign has actually run on the
integrated candidate. Record the candidate commit/tree, workflow run, gate summary,
and explicit release-level acceptance here before closing #46.
