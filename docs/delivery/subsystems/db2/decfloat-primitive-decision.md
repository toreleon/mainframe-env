# Finite DECFLOAT assignment primitive

Status: **Private prerequisite decision; public integration pending**
Owner: **Db2 provider maintainers**
Scope: **DB2-1202.finite-decfloat-assignment**
Applies from: **db2.core development**

## Adoption boundary

Reuse the workspace-pinned `dec = 0.4.11` solely for bounded IEEE decimal
conversion and seven-mode rounding behind the existing numeric-value owner.
This is a standards primitive, not an SQL engine, COBOL policy, or native IBM
fallback. The provider consumes only opaque INTEGER/BIGINT/DECIMAL literal
proofs and resolved DECFLOAT(16/34) targets, checking existing assignment
compatibility first. No raw SQL input, arbitrary value/type constructor, special
or exponent literal admission, HFP, arithmetic, default binding or execution
route is added. Existing exact numeric assignment guards remain unchanged.

## Inspected dependency

Before adding the provider edge, inspected the exact local published crate
manifests, README, CHANGELOG, license, context/status implementation, fixed-width
parse/coefficient/exponent accessors and `decnumber-sys` C build script.
`dec 0.4.11` is Apache-2.0, Rust edition 2018, with no declared `rust-version`;
upstream MSRV is therefore unspecified, not inferred from edition. Its bundled
changelog records 0.4.11 on 2025-05-06 and earlier memory-safety repairs in
0.4.9. Inspection establishes that snapshot only; current upstream maintenance
could not be established by the unavailable repository web page. No maintenance
or security guarantee is claimed. Repository Rust 1.95 acceptance and fresh
dependency/advisory/license/source checks are required for this edge.

Use the ordinary workspace dependency without optional `serde` or `num-traits`
features. The existing locked closure is `decnumber-sys 0.1.6` (ICU),
`libc 0.2.189`, `paste 1.0.15`, `static_assertions 1.1.0`; the C build uses
`cc 1.4.4`, `find-msvc-tools 0.1.11` and `shlex 2.0.1`. No package version or
source changes are authorized. All sources remain the locked crates.io registry.
The `dec` checksum is
`09ae126ef80702bd514a66279b6dac1e58ff1dbcb12024f7110de95608348fd7`;
the `decnumber-sys` checksum is
`23b4bc33814bd5bcd46dd13f9471a29ab1a22c4701ae0c4a182e45e8336d1a5b`.
[ADR-0008](../../../decisions/0008-icu-license-compliance.md) already approves
this exact ICU chain and owns full distribution notices. This decision adds no
license exception. The C build compiles bundled decNumber files with explicit
target-endian configuration; a working target C compiler is required. Current
host verification establishes only the tested host, not every platform.

## Bounds, failure and deterministic rounding

Derive a bounded primitive input from the proven signed coefficient and scale,
never from respelled untrusted SQL. At most 31 coefficient digits and scale 31
fit fixed stack formatting and a bounded NUL-terminated parser buffer. Fixed
Decimal64/Decimal128 storage performs the conversion; no binary float
intermediate or competing rounder is allowed. Every call starts with a fresh
context and sets the caller's explicit mode before parsing. Only Ceiling, Down,
Floor, HalfDown, HalfEven, HalfUp and Up are admitted; ZeroFiveUp is unavailable.
Independent fixed vectors cover both signs, ties, carry and precision loss.

Reject parser failures, allocation reservation failures, exceptional status,
nonfinite/special output, overflow, underflow, invalid context/operation and
unexpected output precision/exponent/sign. Only bounded inexact/rounded
observations may escape as owned facts; no SQLCA warning policy follows.
Rust's allocator may still abort on an allocation the third-party CString
implementation performs; a bounded buffer and fallible reservation reduce this
surface but do not make process OOM recoverable. Fixed-width C parsing does not
use the arbitrary-precision arithmetic allocation path. Build/C tool failures
fail verification; they never select a native oracle or another engine.

Return owned coefficient, exponent, canonical sign, declared precision/target
nullability, original source proof/span and explicit rounding origin. Preserve
admitted exact zero quantum and trailing-zero exponents. Source materialization
already canonicalizes negative zero; assignment cannot reconstruct it.
Third-party types, contexts, status flags and raw IEEE bytes stay private and
never become checkpoint, catalog, evidence, SQLCA or stable wire authorities.
Db2 DECFLOAT column sizes are 9/17 bytes; the primitive's 8/16-byte IEEE layout
does not establish a Db2 wire representation.

## Source and applicability

Baseline `ibm-db2-for-zos-13-2026-08-13`, product `SSEPEK_13.0.0`; paths below
are full pinned topic identities. Retained paths were checked first and absent;
exact manifest byte counts/SHA-256 matched the local raw archive and were read
with the existing `ibm_docs.plain_text(bytes)` parser. Normal search/read remains
TOC-blocked. No network refresh, raw publication commit or execution credit.

| Topic path | Bytes | SHA-256 |
|---|---:|---|
| `SSEPEK_13.0.0/sqlref/src/tpc/db2z_numericassignments.html` | 18430 | `3f6ba8a8290190c2e36590aa348fb816fff6a166c7483b97d1b51cc53bbd8302` |
| `SSEPEK_13.0.0/sqlref/src/tpc/db2z_currentdecfloatroundingmode.html` | 6515 | `f560f8647556aa0d8b8c56f96cbcfae7cbab9edb694a5ecc24f8382568c86932` |
| `SSEPEK_13.0.0/sqlref/src/tpc/db2z_decfloatroundmode.html` | 1723 | `edeb3384e9752fa02caf746884d51e8c47e5765cb526481c8f0d4cd9d81122cf` |
| `SSEPEK_13.0.0/sqlref/src/tpc/db2z_bif_decfloat.html` | 21071 | `22ef060c92f35e04190a11ac5403b0a196dbf5961b36db0f3524987dacc0d74b` |
| `SSEPEK_13.0.0/sqlref/src/tpc/db2z_datatypesintro.html` | 22904 | `a488006755eedd9ef58da3ba8ef9f304a3d79c3910cc39da637dda1f3c38f570` |
| `SSEPEK_13.0.0/sqlref/src/tpc/db2z_constantsintro.html` | 17915 | `bf0cb79eac0636348209b6919c4f4ee680f1c3d2ada9cab186dddfdd39a13fa8` |
| `SSEPEK_13.0.0/sqlref/src/tpc/db2z_assignmentandcomparison.html` | 40605 | `2cc975449a25d1d825cbd8a6551f5ea9c6dfc6ed5dc9956643f0e1dc9948af8a` |

These are language elements, with no standalone statement-catalog row. They
inform later SQL0050 defaults and other assignments without completing those
rows. Numeric assignments specify integer temporary DECIMAL(11,0)/(19,0),
target precision and the static bind/native procedure versus dynamic/static
CREATE VIEW register distinction. Initial register mode derives from package,
native procedure or installation context, not a universal HalfEven. The tiny
rounding-mode topic also requires compatible stored environment information
across referenced views/functions; catalog integration is pending.

Caller-declared context records provenance intent, not proof that package,
procedure, register selection or installation inheritance has been integrated.
Manager-owned exports and independent public tests remain pending, as do
default applicability, ordinary family closure, typed catalogs, SQL execution,
exact 174-row common/deferred freeze and reviewed-rule acceptance. Licensed
Db2 differential remains pending; source review and unit tests grant zero
official row/gate credit. Backend, authorization, restart/recovery and migration
routes are unchanged by this nonmutating kernel.

## Removal and handoff

The adapter isolates the primitive behind owned outputs. Removal requires a
separately reviewed replacement with the same source-backed fixed vectors and
dependency gates; unsupported conversion must fail explicitly until then.
There is no silent fallback. The new decision document requires the manager to
regenerate the shared documentation manifest before public integration; that
generated file is outside this worker's exact ownership.
