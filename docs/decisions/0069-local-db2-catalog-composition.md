# ADR-0069: Owned local Db2 catalog composition

Status: proposed finite first-party contract; implementation and controls unrun
Owner: application composition and Db2 storage maintainers
Scope: signed selected catalog in one genuine local provider-state transaction

Applies from: mainframe-env 0.2.0 finite local Db2 application composition

## Context

The existing application package authenticates `data/db2/catalog`, complete original
Db2 table definitions and typed SQL table/seed sections. The existing server decoder
already joins names, ordered columns/nullability and primary keys, and derives the
catalog application/generation/identity from the genuine retained signed package.
Sequential catalog installation cannot join seed, CICS and RACF writes atomically.

## Decision

The optional finite local participant uses those existing exact signed bytes and
that existing decoder. No new schema/default/encoding or SQLCA behavior is inferred.
Full widths, result encodings, defaults, foreign keys and extract layouts remain
explicit in the original table-definition codec. All authenticated SQL seed values
are used by the original catalog transition and its unchanged refusal rules.
SQL sections without an owned signed catalog refuse. Other unsupported subsystem
families remain unsupported until their concrete ownership participants exist.

The participant holds the real Db2 durable state lock after CICS/dataset preparation
and before the RACF writer, matching Db2 execution's existing authorization direction.
Before preparing it observes all eleven actual row-store namespaces through bounded
store APIs, validates namespace/key/version/order and the real row-store manifest,
and joins the complete decoded state and every provider version to the held cache.
Legacy single-row state migration is not performed by this new preparation path.
Normal legacy catalog and migration APIs remain outside this finite admission.

Preparation uses the unchanged prospective install/rollback transitions. It offers
every original row-codec expected-version Put to the same ProviderCompositionCommit
as the seed/CICS/routes/installer/RACF writes. It does not persist or publish cache
before a genuine acknowledgement covers the exact same Arc and all prepared writes.
Unknown persistence or acknowledgement quarantines this live provider and the local
server until reopen decodes authoritative durable state. Ordinary failed CAS retains
its original known failure semantics; it supplies no Ready receipt.

This Put-only profile requires the same table-name set across selected generations.
It refuses adoption of preexisting tables without an owned previous projection and
foreign CURRENT catalog ownership before the original exact-retry fast path, even
when legitimate runtime deletion has made the foreign table empty. Removing owned
names would require a separately reviewed Delete acknowledgement and is not silently
implemented here. Unselected retained history does not create foreign CURRENT owners.
The original legacy installer/adoption/rollback bodies remain unchanged.

The local selection retains application, generation, signed identity and complete
static table definitions. Readiness compares these against the actual selected and
retained catalog and provider provenance. Runtime rows, pending units, cursors and
replay remain full genuine provider payloads, rather than static readiness aliases.
An own exact retry preserves runtime edits and emits no Db2 write. Retained rollback
uses the original catalog snapshot captured from real runtime state. Existing busy
pending-unit/cursor, schema compatibility, key, foreign-key and capacity checks remain.

The new admission bounds all observed namespace keys/payloads to 16 MiB, current and
prepared canonical state together to 32 MiB, and participating writes to 16 MiB;
original provider/row/catalog limits and the 64 MiB acknowledgement bound still apply.
Large unrelated retained history can therefore refuse this new finite profile; it
does not change legacy provider capacity. Bounds are not aggregate RSS assertions.
Observations and CAS are same-service ownership evidence; trusted direct store
administration or another service instance is not claimed to be excluded globally.

## Verification and scope

Seven proposed Memory/SQLite controls cover full schema/seed/Ready reopen, genuine
runtime DML then unchanged retry/upgrade/rollback, foreign CURRENT after emptying,
signed section/full-schema/seed refusals, before/after Unknown and foreign-Arc
acknowledgements, atomic SQLite companion conflicts, and actual pending-unit,
capacity, stale projection and physical store-version refusals. All original
seventeen Source/seed/CICS/RACF controls remain byte-exact. Controls are unrun.

This finite local implementation supplies no CD009/CD016/Foundation, full V3,
licensed execution, IBM semantics or vendor-equivalence credit. Original complete
package, phase, full-record, reopen, rollback and raw-oracle requirements remain.
