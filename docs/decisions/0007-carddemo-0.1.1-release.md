# ADR-0007: Release mainframe-env 0.1.1 after CardDemo certification

Status: **Accepted by repository owner**
Owner: **repository owner**
Scope: **CardDemo-full 0.1.1 release promotion and naming authority**
Applies from: **mainframe-env 0.1.1**
Date: **2026-08-31**
Supersedes: the release-order hold in ADR-0006

## Context

The generic mainframe-env implementation has been certified with the CardDemo
corpus across 27 passing issues, 20 passing journeys, and clean memory, SQLite,
PostgreSQL 18, Zowe CLI, restart, security, overload, backup, and supply-chain
evidence. CardDemo is a conformance workload, not a shipped product feature or
production dependency. The repository owner authorized promotion to 0.1.1 and
corrected a corpus-tooling typo: the application name is `CARDDEMO`, not
`CARDEMO`.

The pinned upstream repository itself contains `AWS.M2.CARDEMO.FTP.TEST` in
three JCL files. Those bytes cannot be rewritten without changing the accepted
corpus identity.

## Decision

- Promote the product, workspace crates, release metadata, and generated local
  release artifacts directly from the unreleased `0.1.0-alpha.0` development
  identity to **0.1.1**.
- Release only the generic `core-server` production profile. Keep CardDemo
  sources, fixtures, application packaging, operator flows, and cumulative
  commands in the non-production conformance/tooling boundary.
- Use `CARDDEMO` in every product-controlled identifier, including
  `CARDDEMO_CORPUS_DIR`, `AWS-CARDDEMO`, correction IDs, documentation names,
  selectors, and the canonical FTP compatibility dataset.
- Retain `AWS.M2.CARDEMO.FTP.TEST` and its derived backup names only as bounded
  compatibility aliases for the exact pinned upstream JCL. New owned routes use
  `AWS.M2.CARDDEMO.FTP.TEST`.
- Generate reproducible local 0.1.1 release artifacts and create the annotated
  local tag `mainframe-env-v0.1.1` only after all release gates pass.
- Do not push the commit or tag and do not publish or deploy artifacts without
  a separate remote-action request.

## Consequences

- 0.1.1 is the first locally released generic product identity; the earlier
  alpha was never published and remains historical evidence only.
- Passing CardDemo evidence certifies reached behavior but does not make the
  sample application a supported product feature.
- The misspelled environment variable and product identifiers are not part of
  the released public contract.
- The upstream alias is explicit, tested, and cannot expand into a general
  misspelling compatibility policy.
