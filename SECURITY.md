# Security policy

Status: **Development disclosure policy**

`mainframe-env` does not currently claim production readiness or a security
support SLA. Security reports are nevertheless handled as private,
stop-the-line engineering work for the current development branch and latest
published release.

## Report a vulnerability

Use the repository's
[private GitHub security advisory form](https://github.com/toreleon/mainframe-env/security/advisories/new).
Do not open a public issue for a suspected vulnerability and do not include
real credentials, customer records, licensed IBM content, or exploit traffic
against systems you do not own.

Include, when available:

- affected commit, tag, crate, route, and configuration;
- prerequisite access and trust boundary;
- minimal reproduction or failing test;
- confidentiality, integrity, availability, durability, and replay impact;
- whether a mutation may already have committed;
- logs with secrets and protected data removed; and
- a suggested fix or containment boundary.

If private advisories are unavailable, contact the repository owner through a
private channel already established for the project. Do not fall back to public
disclosure merely to obtain a tracking number.

## Handling expectations

Maintainers should acknowledge receipt, reproduce against an exact candidate,
classify impact, and agree on disclosure timing. There is no guaranteed response
window while the project remains pre-production. A fix is not complete until it
has a focused regression, the affected security/durability gates pass, release
notes describe compatibility impact, and any published vulnerable artifacts
have an explicit disposition.

## Security model

The normative security boundaries are documented in:

- [Security and capability architecture](docs/architecture/PLUGIN-AND-SECURITY.md)
- [Execution and durability](docs/architecture/EXECUTION-AND-DURABILITY.md)
- [Canonical effect encoding](docs/contracts/EFFECT-CANONICAL-V1.md)
- [Pre-0.9 deep review](docs/reviews/PRE-0.9.0-DEEP-REVIEW.md)

Local conformance success, historical evidence, or a green ordinary test suite
does not by itself establish production security or licensed IBM equivalence.
