# Generated common-program and route registries

Status: **Frozen for mainframe-env 0.2.0**

The reviewed `mainframe-env.common-program-catalog@1` is the single name-to-
type boundary for JES utilities, nested Db2 TSO programs, and installed COBOL
system services. Generation emits immutable entries and typed builtin, TSO,
system-service, disposition, and execution enums. Runtime performs one exact
registry lookup and dispatches only on the typed result. Unknown workload
programs continue through `ProgramService`; cataloged unsupported programs fail
explicitly. No application identity or business behavior is a common-program
exception.

Official z/OSMF registration is generated from the frozen 23-route owned
catalog plus a reviewed handler binding file. Custom CICS session methods live
in a separate seven-method `mainframe-env-custom` catalog. Official IDs must
begin `/zosmf/`, custom IDs must begin `/mainframe-env/`, the sets must be
disjoint, every handler must exist, and generated output embeds input digests.
The gateway composes the two generated routers and contains no handwritten
route registration.

`cargo xtask program-registry --check`, `cargo xtask route-registries --check`,
and `cargo xtask dehardcoding --check` reject stale generation, denominator or
namespace drift, production application identities, and reintroduced string
dispatch. Generated catalog/route presence grants zero official compatibility
coverage credit.
