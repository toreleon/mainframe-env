# Hardening review #46 — integrated local acceptance

## Status

Accepted for tracker closure on 2026-09-07 under the controller-approved local-CI policy. Hosted GitHub Actions is not used as the closure authority for this acceptance.

All eleven independently reviewable findings tracked by #47 through #57 are implemented, merged, and closed. The published `mainframe-env-v0.8.2` tag predates the final #57, #56 disposition, and #54 assurance-tier merges, so that tag is not retroactively treated as evidence for the complete hardening set.

## Exact local candidate

The final offline campaign passed on:

- commit: `4fb596a5589d53511e624d90d8759302b5063af4`
- tree: `1816b2dc8e28a8e6813fa9e84a6beaf26420c150`
- `rustc 1.98.0 (88d9e12ae 2026-08-18)`
- `cargo 1.98.0 (797e8a9bc 2026-08-05)`
- Linux x86-64 sandbox using the locked offline Cargo vendor bundle

The source worktree was required to be clean before and after the campaign.

## Local campaign result

The exact candidate passed:

- `cargo fmt --all -- --check`;
- 12/12 assurance-selector unit tests;
- `cargo xtask spec --check`;
- `cargo xtask schemas --check`;
- `cargo xtask cobol-exit --check`;
- `cargo test --workspace --all-features --locked --no-fail-fast`;
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`;
- `cargo check --workspace --all-targets --all-features --locked`;
- warnings-denied workspace documentation;
- `cargo xtask architecture --check`;
- `cargo xtask conformance --check`;
- `cargo xtask evidence seal --check`;
- `cargo xtask runtime-architecture --check`;
- store unit tests;
- memory/SQLite Move contract tests;
- memory/SQLite canonical-effect contract tests; and
- the bounded behavioral-mutation campaign: 4 killed, 0 survived, 0 invalid, 0 timed out.

The sandbox has no PostgreSQL server or container runtime, so PostgreSQL cases are not relabeled as local passes. The already reviewed #51/#57 PostgreSQL receipts remain the backend-specific evidence; skipped PostgreSQL tests receive no new evidence credit here.

## Closure findings

The integrated campaign found repository-level assurance metadata drift after the child work had merged and corrected it in the accepted local candidate. Those corrections do not broaden licensed-equivalence claims or substitute local evidence for IBM differential evidence.

## Evidence boundary

This acceptance closes the September 5 correctness/evidence hardening tracker. It does not grant licensed IBM equivalence. Local, model, GnuCOBOL, synthetic, and reference evidence remain distinct from licensed differential evidence, and the existing licensed campaigns remain pending under their own certification gates.
