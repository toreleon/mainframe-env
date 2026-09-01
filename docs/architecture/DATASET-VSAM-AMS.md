# Dataset, VSAM, catalog, and AMS authority

Status: **Normative from 0.6.0**

## Authority boundary

`mainframe-env-dataset` owns dataset definitions, catalog relationships,
allocation, deterministic abstract volumes, lifecycle, record/index state,
locking, RLS/TVS decisions, and dataset recovery. The host contract carries
owned bounded DTOs. SQLx provider-state transactions and artifact storage are
replaceable persistence adapters; their locks, transaction result, object key,
or filesystem layout do not define VSAM semantics.

AMS is a language and orchestration adapter over these typed operations. It
does not own a catalog database, allocation table, record representation, or
recovery log. JCL, CICS, z/OSMF, application installation, and AMS observe the
same selected dataset authority.

## Frozen programming surface

The normative detailed inventory is
`conformance/0.6/inventory/dataset-programming-surface.json`. It contains 131
typed surface descriptors in ten families, including all 31 frozen AMS command
rows and exact mappings for the five official VSAM organization rows. It adds
obligations beneath the immutable 0.2 denominator; it does not invent new IBM
coverage rows.

`cargo xtask dataset-contract --check` compiles the Draft 2020-12 schemas,
checks the 36 official mappings, validates the exact family counts and command
order, and verifies the generated Rust descriptor registry. Catalog presence
or descriptor generation grants no behavior coverage.

## Definition and capability contracts

`mainframe-env.dataset-definition@1` composes:

- organization, record format, LRECL, key, and CCSID attributes;
- block size and bounded buffering;
- primary, secondary, directory, and extent policy;
- ordered volume selection and unit count;
- SMS class and extended-format/addressability attributes;
- CI/CA, SHAREOPTIONS, spanned, reuse, erase, write-check, buffering, and
  striping attributes;
- compression and encryption-key references;
- catalog type, catalog selection, owner, expiration, and retention; and
- lifecycle, migration level, and backup generation.

The deterministic provider advertises abstract-volume allocation/extents,
buffer reservation, SMS-class placement, extended-format/addressability,
catalog metadata, CI/CA, RLS, bounded SHAREOPTIONS, and KSDS TVS support.
Physical disk, tape, ACS, encryption, compression, striping, and
migration/recall remain false until their adapter or semantic work package
passes. An operand
that requires a false capability returns `UnsupportedCapability` with both the
capability and affected operand; it cannot be silently stored, ignored, or
reported as successful.

## Durable state and migration

Dataset state writer version 6 (`MEDS6`) is a bounded owned binary codec. The
reader accepts `MEDS1` through `MEDS6`. Older records materialize reviewed
compatibility defaults and are fully validated before any current-version write.
Corrupt, over-limit, cross-reference-invalid, or capability-invalid state fails
before publication.

The portable state, diagnostic, capability, and migration schemas live in
`conformance/0.6/schemas`. The non-destructive migration contract is
`conformance/0.6/migrations/dataset-state-v2-to-v3.json`,
`dataset-state-v3-to-v4.json`, `dataset-state-v4-to-v5.json`, and
`dataset-state-v5-to-v6.json`. MEDS4 records materialize `NONRLS`; MEDS5 persists
the explicit access mode, and MEDS6 persists catalog creation dates for
retention enforcement. Rollback restores a
digest-verified pre-migration provider snapshot atomically; partial migrated
state is never selected. Backup/restore and broad injected-failure evidence belongs
to DAT-606.

## Deterministic allocation model

The abstract adapter calculates capacity from declared units using fixed
contract constants: 56,664 bytes per track and 849,960 bytes per cylinder;
block allocations use explicit BLKSIZE or LRECL, and record allocations use
LRECL. These values model deterministic allocation behavior only. They do not
claim a real device geometry, VTOC placement, tape operation, compression
ratio, encryption, or stripe layout.

Primary and secondary quantities produce ordered observable extents. CONTIG
collapses them into one extent, ROUND rounds the selected total to the abstract
cylinder constant, and RLSE on lifecycle close releases unused secondary space
without shrinking below primary. BUFNO and BUFSIZE produce a bounded reserved
buffer byte count. SMS class names, guaranteed-space, extended format,
extended addressability, and abstract unit count feed a stable placement
identity; guaranteed space rejects an over-limit definition before publication.
Explicit volume names remain abstract placement labels unless a physical-volume
adapter is declared.

## Organization and access model

The five frozen VSAM organizations select distinct state and access rules:

- KSDS owns sorted unique primary-key records; key access and sequential access
  return the primary key as identity.
- ESDS preserves arrival order. Its deterministic data-component RBA is the
  cumulative byte length of preceding logical records. Reads require an exact
  record-start RBA and RBA rewrites preserve record length.
- LDS stores one bounded logical byte stream behind chunked durable storage.
  RBA reads and writes cross chunk boundaries without exposing chunks, and
  sparse writes are rejected.
- fixed RRDS maps exact-length records to one-based RRNs; empty slots do not
  renumber later records.
- variable RRDS uses the same stable one-based RRN authority while enforcing
  the declared maximum logical record length.

Explicit sequential access uses a zero-based logical position and a direction;
returned identities remain organization-specific (key, RBA, RRN, or ordinal).
All mutating RBA/RRN/key paths publish data and idempotency result in the same
provider-state transaction, and restart reconstructs the same identities.
CI placement accounts for seven deterministic control bytes per interval.
Nonspanned records must fit one interval; VS/VBS records retain one logical byte
string while their geometry reports the exact number of occupied fragments.
Control areas contain an integral declared number of intervals. The description
reports occupied CI/CA counts and high-used RBA without exposing a page manager.

## Locking, RLS, TVS, and recovery

RLS uses durable dataset or organization-specific record identities. A logical
tick is supplied by the request, leases expire only against explicit ticks, and
RLS data mutation requires a live covering update or exclusive receipt owned by
the invoking principal. Resources are acquired in lexical dataset/target order;
an inversion returns `LOCKORDER` before publication. The deterministic adapter
implements SHAREOPTIONS `(1,3)` and `(2,3)`; other combinations fail explicitly
instead of being stored without behavior.

TVS currently accepts KSDS operations. A durable UOW stages inserts, rewrites,
and deletes while holding exclusive primary-key locks. Commit publishes every
touched base cluster, upgraded AIX/PATH version, UOW state, released lock, and
idempotency replay in one mixed provider-state transaction. Rollback publishes
only the terminal UOW and releases its locks. An injected pre-commit failure
marks the UOW `Unknown`; an owner-scoped reconciliation request explicitly
chooses commit or rollback. Restart reloads active and unknown UOWs and validates
all lock/UOW/dataset cross-references before provider publication.

## Catalog, GDG, and partitioned-directory model

One bounded `DependencyGraph` owns every relationship edge. Edges point from a
dependent to its authority: alternate index to base, path to alternate index,
catalog alias to target, generation to GDG base, PDSE member alias to member,
and migrated generation to source. Edge insertion rejects cycles before durable
publication. Reverse invalidation is deterministic, dependent-first, and is
used to reject rename/delete while an unsupported dependent cleanup would be
left behind.

Catalog resolution follows exact aliases first, then the longest qualifier
alias that names a connected user catalog, then an explicit catalog recorded in
the dataset definition, and finally the connected master catalog. Resolution
returns the requested name, resolved name, selected catalog, bounded alias
chain, and maximum participating version. Disconnected catalogs fail with an
exact catalog condition; they do not silently fall through to another catalog.
Dataset catalog owners are enforced against the invocation principal. Creation,
expiration, and retained-day metadata persist in MEDS6. Non-purge delete must
supply a valid current Julian date and fails `PROTECTED` before expiry; PURGE
still requires the owner and is explicit in the request digest.

PDS retains one bounded member record stream per directory name. PDSE uses a
separate ordered generation list per member; normal member reads select the
latest generation and an explicit non-positive relative generation selects
history. Each generation records whether it is a program object. Member aliases
resolve to the same generation authority and cannot shadow members or form
cycles. `MEDS6` persists generations and aliases without duplicating a selected
member projection.

GDG roll-in distinguishes `SCRATCH` from `NOSCRATCH`: both remove old
generations from relative selection, but only `SCRATCH` removes dataset state.
`EMPTY` retires every prior active generation when the limit is exceeded;
`NOEMPTY` retires only the overflow. Alias-dependent scratch is rejected before
mutation until atomic dependent cleanup is available.

## Generated AMS language and handlers

`conformance/0.6/ams/grammar.json` is the 31-command modal grammar inventory.
`cargo xtask dataset-contract` validates it against the frozen AMS command order
and the detailed programming surface, then generates the batch grammar table.
The parser owns bounded continuation, parenthesis, command/subcommand, `IF
MAXCC|LASTCC ... THEN`, and `SET MAXCC|LASTCC` forms. It parses and validates the
entire control stream before the first effect, so an unknown later statement
cannot leave earlier catalog mutations behind.

Every generated command ID has a dispatch path. Required commands invoke typed
dataset, catalog, AIX, lifecycle, or bounded snapshot operations; tape library,
tape volume, page-space, and controller-cache commands return an explicit
capability condition. Command failures update `LASTCC` and the monotonic `MAXCC`;
modal statements may reset either code, and the final maximum becomes the
IDCAMS program return code rather than an infrastructure failure.

AMS dataset operands are authorized through the shared principal/resource
authority before host effects. `BLDINDEX` is a durable AIX rebuild/version
transition rather than a no-op. `UPGRADE` and `NOUPGRADE` are represented in
typed `MEAIX4` state: upgrading indexes are atomically rebuilt with base
mutation, while non-upgrading indexes retain their materialized identity map
until `BLDINDEX`. Export/import uses a bounded provider-neutral header followed
by exact records and never exposes the provider codec. Export-disconnect/
import-connect atomically use catalog connection transitions.

## Evolution rules

- Writers emit only the current state version; readers support the documented
  finite range.
- A new accepted operand extends the normative inventory and generated
  registry before public handling is added.
- Every operand changes definition, allocation, catalog, volume, lifecycle, or
  access behavior, or fails with an explicit capability diagnostic.
- Catalog aliases, GDGs, AIX/base relations, migration ordering, and
  invalidation reuse one bounded dependency-graph authority.
- Dataset names and initial layouts remain application-package data; provider
  code cannot dispatch on them.
