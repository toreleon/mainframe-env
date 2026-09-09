# Engineering reviews

Status: **Current index**

Engineering reviews capture findings against an exact candidate. They are
decision inputs, not conformance evidence, and they remain distinct from
historical release receipts under `conformance/`.

| Review | Candidate | Disposition |
|---|---|---|
| [Pre-0.9.0 deep review](PRE-0.9.0-DEEP-REVIEW.md) | `1bd294c` | No-go until all P1 findings close |

When a finding is repaired, preserve the original review and add resolution,
regression, candidate, and validation references. Do not rewrite an unfixed
finding as closed merely because a broad test suite passes.
