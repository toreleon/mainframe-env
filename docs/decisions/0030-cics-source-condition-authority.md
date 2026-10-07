# ADR-0030: Common CICS response authority in source validation

Status: **Proposed; private source-contract validation**
Owner: **CICS contract and verification maintainers**
Scope: **private source-validator dependency and condition identity**
Applies from: **mainframe-env development**

## Decision

The private SPI/FEPI family validator must pair each declared response condition
with its RESP number. Schema shape alone permits a valid name with another
condition's number. Use the existing generated application condition records,
shared IR condition names and domain-separated authority digest. Verify their
profile, table identity and exact topic pin against the application manifest
before validating response pairs. Reject unknown aliases and wrong numbers.

Promote xtask's existing IR workspace dependency from dev to normal dependencies
and declare that single edge in the additive v0.10 dependency inventory. The
architecture guard checks the real Cargo graph including that inventory. No
package, version, lockfile change, profile addition or layer exception is needed.
The verification tool consumes the common authority; runtime routing and provider
state stay with their existing owners.

## Consequences and source scope

There is no second committed condition table or response mapper. An ephemeral
read-only lookup derives only from verified common records. RESP2 remains
command-specific; null, opposed claims, case-only NOTFOUND and recovery questions
are preserved rather than normalized. The 2700 existing response clauses already
agree, and their family artifacts and grammar projection remain byte-identical.

Primary authority: ibm-cics-ts-6x-application-api-sources-a-2026-09-10,
SSJL4D_6.x/reference-diagnostics/eib/dfhp4_eibfields.html, SHA-256
`a342c35cc115f34f07c0f596f24dc203f3e51e2ec765da5954e4c4c0f3eed8e7`,
table `dfhp4au__table_nt5_kw4_b1c`. Offline search/read of parser lines777-1056
covers all121 condition records and the EIBRESP2 qualifications. Per-command
SPI/FEPI catalog rows and pins remain unchanged in their family contracts.

Negative regressions reject changed authority records, order, digest and source
pin, mismatched name/code and unknown aliases. Positive preservation checks keep
RESP2 null and opposed claims intact. Actual family instances and mandatory
policy/architecture gates are required before sealing this source-only child.
This earns no execution, application dependency, recovery or licensed credit;
parent SPI-1001 and command acceptance remain Pending.
