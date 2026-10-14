# ADR-0053: Declare COBOL floating storage profiles and checkpoint interpretation

Status: Accepted for the authorized #160 implementation; licensed differential pending
Owner: COBOL compiler and interpreter maintainers
Scope: COMP-1/COMP-2 layout attributes and reference-machine checkpoint profiles
Applies from: mainframe-env issue #160

## Decision

New compiler artifacts append `float_profile = "hex"` to floating layout
operations. The layout ABI accepts only named `"hex"` and `"ieee"` profiles;
absent metadata preserves old artifacts as IEEE. One interpreter codec owns
encoding and decoding for value conversion and shared REDEFINES storage.
No source-language compiler option or dependency is introduced.

Floating machines produce checkpoint schema 13, appending a canonical map of
layout names and representation markers after the unchanged schema 12 prefix.
Restoring schemas 1–12 selects IEEE for floating layouts even with a newly
compiled HEX artifact. Their storage bytes are not rewritten. Recheckpointing
records this selection in schema 13. Schema 13 requires an exact floating-name
set and valid profile markers; nonfloating machines continue emitting schema 12.
An older interpreter cannot read schema 13 and must not resume a new checkpoint.

The pinned Enterprise COBOL for z/OS 6.5 baseline
`ibm-enterprise-cobol-6.5-2026-05-31` topics `rlddecom` and `rlddeusa` establish
4/8-byte storage (USAGE row 0015). The authorized #160 decision supplies the
HFP format/default while the FLOAT-option topic remains unavailable offline.
UNVERIFIED ASSUMPTIONS pending licensed differential: truncate 24/56-bit
fractions toward zero, raise SizeError above maximum magnitude, and underflow
to true zero. See the [runtime contract](../architecture/COBOL-RUNTIME-SEMANTICS.md#comp-1comp-2-storage-160)
for the existing 34-digit decimal adapter limit and round-trip policy.
