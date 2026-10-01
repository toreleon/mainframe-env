# 0.4.0 decimal and IEEE semantic-gap spike

Status: **Frozen decision; licensed IBM differentials pending**

Candidate date: 2026-09-01. Toolchain: repository-pinned Rust 1.95.

| Candidate | Version | Result | Decision |
|---|---:|---|---|
| `dec` / libdecnumber | 0.4.11 / `decnumber-sys` 0.1.x | General Decimal Arithmetic contexts expose precision, eight rounding modes, accumulated conversion/division/inexact/overflow/rounded/subnormal/underflow status, configurable exponent bounds, fixed stack capacity, decimal128 canonical bytes, signed zero, infinity, and NaN. Apache-2.0; safe public API over a bundled C reference implementation. | Adopt behind the owned bounded adapter. |
| `bigdecimal` | 0.4.10 | Arbitrary coefficient/scale, explicit precision and rounding, but no IEEE decimal special values or equivalent accumulated trap/status model; allocation is not intrinsically bounded and formatting has configurable ambient defaults. MIT/Apache-2.0. | Reject as the runtime primitive; the compatibility adapter would be larger and less bounded. |
| Native `f32`/`f64` bits | Rust 1.95 | Exact IEEE binary storage width, signed zero, infinity, and NaN payload transport; no owned COBOL status/rounding or cross-platform transcendental contract. | Use only as canonical binary storage/primitive operations behind owned status and intrinsic adapters. |
| `libm` | 0.2.16 | Pure-Rust C-math implementations, MIT, MSRV 1.63. The selected `force-soft-floats` feature avoids target intrinsic selection for the frozen transcendental path. It does not supply COBOL decimal types, conditions, or formatting. | Adopt for bounded transcendental primitives behind owned domain/status/result normalization. |

The executable spike is
`runtime::tests::{decnumber_spike_preserves_precision_rounding_and_status,
prohibited_rounding_and_binary_special_bits_are_explicit,
decimal_floating_canonical_bytes_preserve_special_values}`. It freezes cases
for 34-digit precision, exact division, inexact rounding, prohibited rounding,
division by zero, binary signed zero/NaN bits, and decimal128 signed zero/NaN
canonical bytes.

The decimal dependency is exact-pinned and has no enabled optional features;
`libm` is exact-pinned with only `force-soft-floats`. The decimal dependency's
direct transitive boundary is `decnumber-sys`, `libc`, and `paste`;
`decnumber-sys` compiles the reviewed libdecnumber C reference source. The
repository's pinned-toolchain compile, advisory, license, and source gates are
required. Removal is localized to the runtime adapter: a replacement must pass
this frozen spike plus COBOL packed/zoned/PICTURE/intermediate-precision and
licensed Enterprise COBOL 6.5 differentials before selection.

The primitive does not own IBM semantics. PICTURE editing, packed/zoned/binary
representation, compiler options, field/intermediate precision, receiving-field
scale, `ROUNDED`, `SIZE ERROR`, aliasing, forbidden mutation, CCSID, and exact
IBM-visible bytes remain mainframe-env adapters and conformance obligations.
