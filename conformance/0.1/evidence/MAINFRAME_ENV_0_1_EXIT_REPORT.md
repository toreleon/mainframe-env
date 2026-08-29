# MAINFRAME-ENV 0.1 exit report

Functional result: **PASS** on the ME.V7 candidate based at ME.V6 commit
`c7f0c6d555709ec0bfb7f64866e29c7d2d04dece`. The required ME.V7 completion
commit is the next protocol action after this derived gate.

All eight phase gates derive pass. The 20-package workspace assigns every
accepted selector to one owner and one default authority. The `core-server`
closure contains the 17 declared product packages and zero of 32 excluded
components; OpenMainframe is an out-of-process oracle only.

The certification pack covers 43 frozen COBOL statement families, 22 CardDemo
CICS operation families/174 blocks, the accepted JCL/JES and ten-program set,
dataset/RACF authorities, and 23 z/OSMF routes. Six COBOL and four CICS families
remain explicit oracle-matching unsupported outcomes with no fallback.

Workspace validation passed 118 tests; the separate PostgreSQL 18 migration
test passed; selected current-oracle reproduction passed 765 tests. Release-mode
loads completed 256/256 at 1x, 512/512 at 2x, and 10000/10000 long-run requests,
with zero active requests or sessions afterward and drained shutdown.

SQLite/PostgreSQL migration head is `0001-durable-state`. Dataset, security,
CICS, JES, authentication sessions, execution/events/work/checkpoints,
idempotency, artifacts, backup/restore, and restart evidence pass. `cargo deny`
reports advisories, bans, licenses, and sources all OK.

The alpha release manifest, CycloneDX 1.6 SBOM, checksums, license expressions,
and SLSA/in-toto provenance are under `release/0.1.0-alpha.0/`. No tag, remote
push, package publication, deployment, or oracle mutation was authorized or
performed.
