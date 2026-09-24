# Changelog

All notable changes to mainframe-env are documented here.

## [Unreleased]

### Changed

- Terminal BTS parent completion now deletes settled descendants, their
  activity indexes, and completion-event pools in the same provider-state
  mutation. A live descendant leaves completion unresolved for explicit
  reconciliation, preserving the parent and child rows.

- RESET and DELETE now preserve a subtree with live descendants until their
  RUN work can be reconciled. They also retain pending descendants owned by
  another UOW, while the defining UOW can still delete its own INITIAL child.

- Parent CHECK ACTIVITY now consumes a completed child's completion event in
  an atomic process/event transition. An incomplete child retains the event;
  RESET ACTIVITY restores a consumed event as NOTFIRED.

- Secret CICS payloads now share one zeroizing byte allocation across clones and
  render as redacted values in debug output. This keeps typed credential
  requests transient without changing their canonical bytes.

- Versioned the typed CICS effect-plan codec as `MCEP` v2 with big-endian `u16`
  operation, operand, option, and output tags. Existing tag numbers and
  canonical v1 plan decoding remain intact; new encodings are deterministic v2
  bytes, and malformed or unknown tags fail closed.

### Fixed

- Rebased the ten CICS security-control commands onto the local v0.9
  integration head. Shared command registrations, generated descriptors,
  codec-v2 tags, compiler/interpreter routes, and recovery tests now retain
  all integrated families at 151 typed, 0 legacy, and 112 unready rows. The
  generated descriptor lookup and executable security entries use bounded
  child modules without raising protected module budgets.

- Rebased the six CICS diagnostics commands onto terminal control and updated
  the shared descriptor, compiler, and generated contract boundaries to 127
  typed, 0 legacy, and 136 unready application rows. Existing terminal and
  other family tag values remain fixed.

- Rebased CICS diagnostics onto the integrated event-control family and split
  the 112-entry executable registry into bounded modules. Generated contracts
  and ratchets now report 112 typed, 0 legacy, and 151 unready application
  rows while keeping all existing tag ranges and protected roots.

- Rebased all six typed CICS diagnostics routes onto the integrated web-service
  and counter-control families. Reconciled generated descriptors, codec tags,
  schema ratchets, and compiled routing to 99 typed, 0 legacy, and 164 unready
  application rows while preserving protected module ceilings.

- Aligned the frozen CICS command-contract readiness schema with the integrated
  73 typed, 0 legacy, and 190 unready application routes.

- Moved the typed CICS TSQ NUMITEMS plan output from tag 110, which overlaps
  the ASSIGN extension mapping, to the lane-reserved non-ASSIGN tag 200. An
  exhaustive codec regression now proves that every `CicsOutputName` tag is
  unique and decodes to its original identity.

### Added

- Added the shared versioned BTS process/activity authority for the pending
  lifecycle slice, with UOW-scoped acquisition epochs, atomic pending DEFINE
  publication or rollback, checkpoint references, and durable exact replay.
  This does not yet register BTS lifecycle commands; the application split
  remains 151 typed, 0 legacy, and 112 unready.

- Added bounded BTS child activity definition and lifecycle transitions to the
  shared authority, including incarnation-scoped IDs, atomic index cleanup on
  reset/delete, UOW publication or rollback, and stale checkpoint rejection.
  Public command registration remains pending.

- Added a durable BTS active-run context bound to the process/activity and
  coordinator checkpoint epochs, with restart reads, lease takeover fencing,
  and a retained closed state for replay protection. Public command routing
  remains pending.

- Connected BTS pending process/child publication and UOW acquisition release
  to the CICS syncpoint and uncertain-outcome reconciliation paths. The
  participant is idempotent and owner-fenced; command routing remains pending.

- Bound existing BTS event commands to the shared 52-character lifecycle
  activity identity when a fenced active context is present. RUN input-event
  delivery accepts that exact indexed activity, while closed contexts cannot
  fall back to a legacy event binding.

- Added activity completion events to the existing BTS event pool and made
  child definition, rollback, reset, delete, and forced cancellation update
  their process and event rows in one atomic store mutation. This remains
  shared authority work; public lifecycle command registration is pending.

- Added a versioned BTS RUN outbox and server work generation. Activation,
  input event, request record, and pending-work index commit atomically;
  restart re-admits missing work, leases fence checkpoints, and selected
  online programs complete the lifecycle row. Public RUN registration remains
  pending while source options and failure paths are finished.

- Preserved CICS runtime startup when one BTS RUN work item is terminal but
  its lifecycle request remains pending. Other RUN work recovers; an exact
  retry of the unresolved request still reports an unknown outcome.

- Retained exact DEFINE PROCESS and ACQUIRE effect identities in the BTS
  acquisition row. Same-key retries survive reopen and reject changed inputs;
  a held acquisition reports source INVREQ rather than losing its replay
  identity after UOW settlement. Public command registration remains pending.

- Added a versioned repository-name reservation to the shared BTS authority.
  Process names are unique across process types mapped to the same repository;
  reservation, definition, and rollback or publication move atomically. Public
  command registration remains pending.

- Added durable DEFINE PROCESS NOCHECK acquisition state. Repository-name
  reservation occurs at syncpoint, while known duplicates are reported by a
  commit preflight before UOW intent is recorded. Rollback and reopen retain
  exact replay and leave an existing process unchanged. Public command routing
  remains pending.

- Exposed the held BTS acquisition's process-container scope to sibling
  commands, with root read/write versus descendant read-only access, owner and
  UOW fencing, and an explicit root requirement for ACQPROCESS.

- ACQUIRE PROCESS now resolves its process type before the named process, so
  missing types and missing processes report their distinct PROCESSERR codes.

- Corrected complete-root SUSPEND ACQPROCESS to return the SUSPEND
  `INVREQ 16/14` activity-mode condition; RESUME ACQPROCESS retains its separate
  `PROCESSERR 108/14` condition.

- Counted BTS PROCESS, ACTIVITY, and CHANNEL literal limits by source
  characters so the documented `¬` name character passes provider, compiler,
  and MCEP validation. A compiled lifecycle route exercises a 36-character
  process name through RUN completion.

- Added the RUN TRANSID child-token port with the sibling FETCH/FREE method
  signatures and `cics-bts-child-ownership-v1` row shape. Registration and
  terminal completion are owner-checked and versioned; RUN TRANSID task
  admission and public command readiness remain pending.

- Added a versioned RUN TRANSID request and bounded work outbox. The local
  child token, transaction, inherited principal, and issue-time channel
  snapshot are retained for exact replay; work claims fence completion by
  lease epoch and deliver the result through the child-token port. Server
  attach and public command routing remain pending.

- Reconciled RUN TRANSID restart against the retained child-token result
  before readmitting work. SQLite reopen preserves its channel snapshot and
  outbox; terminal work without a child result remains unresolved while
  independent work recovers, and its exact reconciliation reports an unknown
  outcome.

- Checked new RUN TRANSID channel snapshots against the container-count and
  payload limits before admission. Exact retries retain the original
  issue-time copy when the current channel has changed.

- Extended the versioned RUN TRANSID child snapshot to retain optional CCSID
  and read-only metadata from the task-owned channel port. Existing v1 rows
  remain readable; new base64 snapshots fit the bounded request and invocation
  payloads at the aggregate byte limit.

- Retained the source channel's read-only flag in the RUN TRANSID request row
  and passed it through the child invocation binding. Older request rows
  default to writable when the flag is absent.

- Added the bounded BTS lifecycle host, IR, compiler, interpreter, and CICS
  dispatch scaffolding, plus an installed process-type/transaction catalog and
  selected RUN and RUN TRANSID worker admission. The candidate generated
  registry is 174 typed, 0 legacy, and 89 unready; acceptance gates and
  child-task failure reconciliation remain in progress.

- Bound ACQUIRE PROCESS to its PROCESS and PROCESSTYPE form and compiled CHECK
  CVDA outputs to exact binary receivers. Compiled COBOL now exercises DEFINE
  PROCESS NOCHECK, ACQUIRE, CHECK, RUN, and RUN TRANSID through selected server
  workers. The CICS contract and registration schemas advance to the isolated
  174/89 candidate and 151 exact registrations.

- Added typed CICS `WEB CONVERSE` as one checked client request and bounded
  response operation, with durable replay, dispatch uncertainty, SAF, and
  continuation through WEB RECEIVE. IBM CICS TS 6.x baseline
  `ibm-cics-ts-6x-2026-08-31:api-commands` row `0241` binds
  `dfhp4_webconverse.html` at
  `sha256:326c6b1e2859591c8ab86ed252ba01d3ee57f2a109afacdf7bc7468ee1f9e658`.

- Added typed CICS `WEB RECEIVE` for bounded server and client body buffers,
  durable retain or discard cursors, response metadata, and client header
  inspection after receive. IBM CICS TS 6.x baseline
  `ibm-cics-ts-6x-2026-08-31:api-commands` row `0248` binds
  `dfhp4_webreceive.html` at
  `sha256:4b49e5dec28edd2dc00da545fb368b9d8538f31fa2494597a5fcacf9ecaa401c`
  and `dfhp4_webreceiveclient.html` at
  `sha256:5c610ca807d29a9af74bddfc4936e2921ffef9c7c2dcf724e85ff6a682881c9b`.

- Added typed CICS `WEB RETRIEVE` for the task-owned token from the last
  pending EVENTUAL document send, with documented INVREQ and NOTFND cases.
  IBM CICS TS 6.x baseline `ibm-cics-ts-6x-2026-08-31:api-commands` row
  `0249` binds `dfhp4_webretrieve.html` at
  `sha256:cfc8653835cdc36978993f6b4aeabb2b908267f7a07763b70167a3b95ae32fc7`.

- Added typed CICS `WEB SEND` for durable server response selection and
  checked client request exchange, with staged headers, SAF, replay, and
  explicit post-dispatch uncertainty. IBM CICS TS 6.x baseline
  `ibm-cics-ts-6x-2026-08-31:api-commands` row `0250` binds
  `dfhp4_websend.html` at
  `sha256:f91baca7b4277d6c86ab5517442b6479bb7422733845e254c3583026627b839b`
  and `dfhp4_websendclient.html` at
  `sha256:863d5f1585433c192080c326cdb1f0695350462cfd37f3b7f9ed4a3e72b4488f`.

- Added typed CICS `WEB WRITE HTTPHEADER` with bounded ordered staging for
  server responses and client requests, forbidden generated client headers,
  durable replay and a compiled route. IBM CICS TS 6.x baseline
  `ibm-cics-ts-6x-2026-08-31:api-commands` row `0252` binds
  `dfhp4_webwritehttpheader.html` at
  `sha256:369252b2fd5372d910ec699d0ad7ade37e8dfd2a77fcac8e6dd5f43ec8dd82c3`.

- Added typed CICS `WEB ENDBROWSE` for header, query, and form cursor release,
  with atomic deletion/replay and a compiled route. IBM CICS TS 6.x baseline
  `ibm-cics-ts-6x-2026-08-31:api-commands` row `0242` binds
  `dfhp4_webendbrowseformfield.html` at
  `sha256:4999d8ef5cb0a9388c43e45eccd79fc7b29e9f465f9457f263fdccea67def1e3`,
  `dfhp4_webendbrowsehttpheader.html` at
  `sha256:28beba13ed2772907ce3144606eade62a2c82de0d2c4202c2853e1409a529f77`,
  and `dfhp4_webendbrowsequeryparm.html` at
  `sha256:4e4734be485e27491c4c1e0e343c391c9a54df3319cb2bddfceafcc6d044080e`.

- Added typed CICS `WEB READNEXT` over durable header, query, and form browse
  snapshots, preserving the cursor on short buffers and reconciling an
  uncertain persisted advance. IBM CICS TS 6.x baseline
  `ibm-cics-ts-6x-2026-08-31:api-commands` row `0247` binds
  `dfhp4_webreadnextformfield.html` at
  `sha256:138c7bc282b407692750df5c2902c6a5a9b17133cb1065e2a765762872a688fd`,
  `dfhp4_webreadnexthttpheader.html` at
  `sha256:9aebf7346383316e927a8c5857618808e7409e299c349182a46daa5c5fb80bc8`,
  and `dfhp4_webreadnextqueryparm.html` at
  `sha256:1503686c2b0a4ae0e9f333f148f0bee8aeddf1c6607e5115b5b914a8d614c0b3`.

- Added typed CICS `WEB STARTBROWSE` for header, query, and URL-encoded form
  snapshots, with named starts, durable cursor state and a compiled route.
  IBM CICS TS 6.x baseline `ibm-cics-ts-6x-2026-08-31:api-commands` row
  `0251` binds `dfhp4_webstartbrowseformfield.html` at
  `sha256:1fc8d0fcb8c30e4e56c3b596d1a40104462a6f2800dd6128e8ac0042f973568f`,
  `dfhp4_webstartbrowsehttpheader.html` at
  `sha256:15d1b7bda4e34dd9c0c0be1a461f61bfe18bf21d2b5ad6d47e89a423c21bf9f0`,
  and `dfhp4_webstartbrowsequeryparm.html` at
  `sha256:ff2b1f672cfaa2729c446bdcfb478b21e02f3d68458602fab695ba70e97301e8`.

- Added typed CICS `WEB READ` for HTTP headers, escaped query parameters, and
  URL-encoded form fields, with checked value lengths and a compiled route.
  IBM CICS TS 6.x baseline `ibm-cics-ts-6x-2026-08-31:api-commands` row
  `0246` binds `dfhp4_webreadhttpheader.html` at
  `sha256:f1bc6891f4891da46ef10f847406f1ed8d70c9e8712c1f1f89599f09927747d1`,
  `dfhp4_webreadqueryparm.html` at
  `sha256:5494c4412e6656b35342cd16392e37fb423c657cd0d2aa5c583c3094ab83d9eb`,
  and `dfhp4_webreadformfield.html` at
  `sha256:fed30f534facad037e9dfe724e518c263efed5bf544eccf54080b73e070e4943`.

- Added typed CICS `EXTRACT WEB` as the separately registered synonym of
  `WEB EXTRACT`, with the same checked server/client metadata behavior and a
  distinct compiled route. IBM CICS TS 6.x baseline
  `ibm-cics-ts-6x-2026-08-31:api-commands` row `0076` binds
  `dfhp4_extractweb.html` at
  `sha256:34412c24defd5a6e0063edd6697c2091e6737f9ee5ab13744fde6fea44d591b4`.

- Added typed CICS `WEB EXTRACT` for task-bound inbound HTTP requests and
  durable client sessions, with source length/condition handling and a compiled
  COBOL route. IBM CICS TS 6.x baseline
  `ibm-cics-ts-6x-2026-08-31:api-commands` row `0243` binds
  `dfhp4_webextract.html` at
  `sha256:328947a78cac9afe1efa11df299067f286e2653de9ae29ec5abc1f2bd4df3427`.

- Added typed CICS `WEB CLOSE` with task-owned token validation, selected
  transport release, durable session removal and replay, and source NOTOPEN
  conditions. IBM CICS TS 6.x baseline
  `ibm-cics-ts-6x-2026-08-31:api-commands` row `0240` binds
  `dfhp4_webclose.html` at
  `sha256:141f05170b7d4fc11e9f59b74b1504e89ff814e9006b5a95f0f8a2851d3341cd`.

- Added typed CICS `WEB OPEN` with source-fenced direct-host or installed
  client URIMAP selection, an eight-byte task-owned session token,
  transport-confirmed HTTP version, bounded durable state and replay, and a
  selected compiled route. IBM CICS TS 6.x baseline
  `ibm-cics-ts-6x-2026-08-31:api-commands` row `0244` binds
  `dfhp4_webopen.html` at
  `sha256:10e939b391746c2bf19e08d52bf7e22cdad2270b983d2502b4e009771da7490a`.

- Added typed CICS `WEB PARSE URL` with bounded URL parsing, escaped query
  preservation, IPv4/IPv6 host classification, fullword buffer lengths, source
  conditions, and a selected compiled COBOL route. IBM CICS TS 6.x baseline
  `ibm-cics-ts-6x-2026-08-31:api-commands` row `0245` binds
  `dfhp4_webparseurl.html` at
  `sha256:5abb860d6ff6e6723515e3c537ba7f1974a511f3abad5ae89461608a1aca07b4`.

- Added typed CICS `ROUTE` with full-BMS terminal selection, bounded
  LIST/OPCLASS routing, checked timing, durable delayed delivery, ERRTERM
  notification, SAF authorization, and atomic replay. Source: IBM CICS TS
  6.x application API sources-c, `dfhp4_route.html`, row 0182.

- Added ten typed CICS ISSUE outboard commands with bounded local sequential,
  keyed, relative, and media destinations; durable record and task state;
  QUERY/RECEIVE, NOTE, WAIT, END/ABORT, SAF checks, and atomic replay.
  Source: IBM CICS TS 6.x application API sources-b rows 0110–0137, plus
  sources-a ASSIGN row 0011.

- Added typed CICS `SEND CONTROL` and `SEND PAGE` with durable device control,
  bounded logical-message accumulation, paging, page completion, SET/RETPAGE,
  SAF authorization and replay. `PURGE MESSAGE` now discards an active logical
  message. Sources: IBM CICS TS 6.x application API sources-c rows 0188/0190
  and sources-b row 0148.

- Added typed CICS `RECEIVE PARTN` with authenticated 8775 partition input,
  AID/partition/cursor and length outputs, first-receive and ASIS handling,
  truncation conditions, durable consumption, and atomic replay. Source:
  IBM CICS TS 6.x application API sources-b, `dfhp4_receivepartn.html`, row 0164.

- Added typed CICS `SEND PARTNSET` with durable task selection, base reset,
  registered partition geometry, SAF checks, immediate receive sequencing,
  and atomic replay receipts. Source: IBM CICS TS 6.x application API
  sources-c, `dfhp4_sendpartnset.html`, catalog row 0191.

- Completed thirteen typed CICS event-control rows, including input and
  composite events, timer definition/check/force/delete, reattachment and
  subevent retrieval, TEST EVENT, and SIGNAL EVENT capture specifications.
  Activity and capture state are durable and replayable across memory and
  SQLite; the selected COBOL routes use reserved MCEP v2 tags while v1
  decoding remains compatible. IBM baseline, catalog rows, topic paths, and
  verified SHA-256 identities are recorded in the v0.9 status document.

- Added typed CICS UPDATE COUNTER and UPDATE DCOUNTER with conditional
  compare, one-past-maximum value, source-defined bounds, atomic durable
  replacement, SAF, and fenced replay. IBM CICS TS 6.x application API
  `dfhp4_updatecounter.html`, rows 0226/0227.

- Added typed CICS REWIND COUNTER and REWIND DCOUNTER with conditional
  at-limit reset, optional increment probe, `SUPPRESSED 102`, durable CAS,
  SAF, and fenced replay. IBM CICS TS 6.x application API
  `dfhp4_rewindcounter.html`, rows 0179/0180.

- Added typed CICS QUERY COUNTER and QUERY DCOUNTER for current, minimum,
  and maximum outputs, including normal one-past-maximum reporting and
  source-defined signed fullword LENGERR warnings on wide counters. IBM CICS
  TS 6.x application API `dfhp4_querycounter.html`, rows 0153/0154.

- Added typed CICS GET COUNTER and GET DCOUNTER with atomic current-value
  allocation, increment ranges, inclusive comparisons, REDUCE and WRAP,
  at-limit SUPPRESSED conditions, fullword LENGERR warning, and replay.
  IBM CICS TS 6.x application API `dfhp4_getcounter.html`, rows 0087/0088.

- Added the eight typed v0.9 CICS web-service-control commands: INVOKE SERVICE,
  SOAPFAULT ADD/CREATE/DELETE, WSACONTEXT BUILD/DELETE/GET, and WSAEPR CREATE.
  The bounded local service route uses installed immutable program generations
  and durable channel containers; SOAP faults, addressing contexts, EPR output,
  exact CICS conditions, SAF, audit, replay, and selected COBOL routes have
  focused regressions. New MCEP v2 tags remain in the exclusive web ranges.
  Source: IBM CICS TS 6.x application API sources-b/c, catalog rows 0107,
  0197-0199, and 0259-0262; exact topic hashes are recorded in the v0.9 status.
- WSACONTEXT now resolves ADDRESS, METADATA, and REFPARMS from a complete
  endpoint reference and rebuilds ALL after a partial field replacement.

- Added typed CICS DELETE COUNTER and DELETE DCOUNTER with durable atomic
  removal, missing-counter `INVREQ 201`, update SAF, and owner-fenced replay.
  IBM CICS TS 6.x application API `dfhp4_deletecounter.html`, catalog rows
  0044 and 0045.

- Added typed CICS DEFINE COUNTER and DEFINE DCOUNTER over a durable,
  versioned named-counter pool authority with source-defined bounds,
  duplicate and pool-rebuild conditions, SAF authorization, fenced replay,
  and compiled COBOL routing. IBM CICS TS 6.x application API
  `dfhp4_definecounter.html`, catalog rows 0034 and 0035.
- Added a bounded durable CICS diagnostics authority with a versioned state
  codec, trace-destination configuration, retained diagnostic snapshots, and
  strict malformed/capacity checks. ENTER TRACENUM now writes bounded, durable
  numeric user trace entries with the IBM exception override and exact
  INVREQ/LENGERR response codes. Source: CICS TS 6.x application API sources-a,
  `dfhp4_entertracenum.html`, catalog row 0066.

- Added typed CICS `DEFINE COMPOSITE EVENT` with exclusive AND/OR predicates,
  up to eight initial atomic children, durable child ownership and reevaluation,
  exact missing/invalid child conditions, SAF, replay, and a compiled v2 route.

- Added typed CICS `ADD SUBEVENT` and `REMOVE SUBEVENT` for durable composite
  membership. Atomic input delivery updates child queues and reevaluates
  predicates; removal preserves the child's fire status and both commands
  enforce source-specific conditions, SAF, replay, and compiled v2 routes.

- Added typed CICS `DELETE EVENT` for input and composite events. Deletion
  unlinks child predicates atomically, preserves the children of a deleted
  composite, rejects system and timer events, and replays durably.
- Added typed CICS MONITOR for installed user event points. It retains bounded
  counter, clock, and character-field updates, replays committed results, and
  applies an authorized point definition. Source: CICS TS 6.x application API
  sources-b, `dfhp4_monitor.html`, catalog row 0143.

- Added typed CICS DUMP TRANSACTION with a durable bounded local section
  snapshot, selected FROM and segment bytes, source dump-code suppression,
  run/count DUMPID, checked SAF, and replay after a result gap. Source: CICS TS
  6.x application API sources-a, `dfhp4_dumptransaction.html`, catalog row
  0057.

- Added a bounded local DUMP form with selected provider-owned sections and
  DCT capture through the same durable diagnostic row. Its catalog identity is
  CICS TS 6.x application API row 0056; the committed CICS TX 11.1
  compatibility topic is unavailable locally, so the implementation does not
  claim target-product dump-dataset or system-dump behavior.

- Added a bounded local TRACE switch route. USER, SYSTEM, and EI update the
  durable local trace flags; SINGLE arms one numeric trace entry. The CICS TS
  catalog identity is row 0220. Its CICS TX cross-product topic is unavailable
  locally, so this route is documented as local behavior.

- Added a bounded local ENTER TRACEID route that retains named trace bytes and
  records ACCOUNT, MONITOR, and PERFORM event data durably with SAF and replay.
  Its CICS TS catalog identity is row 0065. The committed cross-product and
  older-version compatibility topics are absent locally, so full IBM
  monitoring equivalence is not claimed.

- Added typed CICS UNLOCK for task-owned no-token and TOKEN update contexts.
  READ UPDATE can return a fullword TOKEN, whose durable counter prevents reuse
  across restart; UNLOCK consumes only the matching task and file token, and a
  missing no-token hold returns NORMAL. TOKEN-based REWRITE and DELETE use the
  same held record authority. Source: IBM CICS TS 6.x application API sources-c,
  `dfhp4_unlock.html`, catalog row 0225.

- Added typed CICS RESETBR for an active default-key file browse. The command
  repositions the existing dataset cursor in place, preserves it on NOTFND or
  ownership failure, invalidates the held update context after success, and
  replays a completed reset without dispatching it again. Source: IBM CICS TS
  6.x application API sources-b, `dfhp4_resetbr.html`, catalog row 0171.

- Added typed CICS `DOCUMENT SET` for individual symbols and symbol lists,
  including case-sensitive replacement, bounded lengths, delimiter and
  UNESCAPED handling, atomic document and replay updates, and compiled EIBFN
  `3C08` routing.

- Added typed CICS `DOCUMENT RETRIEVE` with DATAONLY or bounded tagged output,
  optional CHARACTERSET conversion, MAXLENGTH probing and truncation, exact
  required LENGTH reporting, and compiled EIBFN `3C06` routing.

- Added typed CICS `DOCUMENT INSERT` for text, binary, template, symbol,
  `FROMDOC`, and retrieved-buffer content. Bounded bookmark insertion and
  AT/TO overlay preserve conversion blocks; the document and replay update
  atomically, with DOCSIZE and the documented failure conditions.

- Added typed CICS `DOCUMENT DELETE` for transaction-owned 16-byte tokens.
  Deletion frees durable document storage immediately, records the delete and
  effect replay atomically, and returns the documented NOTFND 13/1 on a new
  request for an absent document.

- Made first-time CICS TDQUEUE-definition registration migration-safe. Existing
  compatibility-profile queues must all be declared and satisfy the proposed
  direction, record-size, record-count, and byte limits before any definition
  row is written; partial or incompatible migrations leave durable and
  in-memory state unchanged.

- Added durable local CICS TDQUEUE definitions for typed WRITEQ TD, READQ TD,
  and DELETEQ TD. Installed intrapartition and extrapartition definitions now
  drive enabled/open direction, record-size and per-queue capacity checks with
  exact DISABLED, INVREQ, NOTOPEN, LENGERR, NOSPACE, IOERR, QIDERR, and QZERO
  conditions across SQLite reopen.

- Added typed local CICS TS 6.x `WRITEQ TS` for row `0258`, including
  QUEUE/QNAME and exact-local SYSID routing, generated or explicit bounded
  LENGTH, append ITEM/NUMITEMS compatibility, ITEM+REWRITE replacement,
  MAIN/AUXILIARY placement, NOSUSPEND capacity handling, exact local
  conditions, update SAF/audit, crash-safe effect replay, legacy-row migration,
  SQLite reopen, and compiled EIBFN `0A02` proof. Remote/shared pools, TSMODEL
  routing, recoverable-UOW coupling, PostgreSQL restart, and licensed
  differential remain explicit pending boundaries.

- Added typed local CICS TS 6.x `READQ TS` for row `0160`, including
  QUEUE/QNAME and exact-local SYSID routing, explicit ITEM and queue-wide
  default/NEXT addressing, INTO or task-owned SET delivery, in/out LENGTH,
  normal-only NUMITEMS, exact local conditions, queue SAF/audit, effect replay,
  legacy-row migration, SQLite reopen, and compiled EIBFN `0A04` proof.
  Remote/shared pools and TSMODEL routing remain explicit fail-closed paths.
- Added typed CICS `DOCUMENT CREATE` over a bounded durable transaction-owned
  document authority. Empty, text, binary, retrieved-document and registered
  template sources produce deterministic 16-byte tokens and optional DOCSIZE;
  symbol lists, host code pages, template READ authorization, atomic replay,
  task cleanup, capacity failure, and SQLite reopen are covered.
- Added typed CICS `TRANSFORM XMLTODATA` with bounded XML metadata query and
  transform modes. The shared XML binding accepts UTF-8 XML and optional CHAR
  namespace declarations, reconstructs fixed BIT-mode data, returns paired
  element/type metadata, enforces resource authorization and source conditions,
  and persists query or output effects for restart replay.

- Added typed CICS `TRANSFORM JSONTODATA` over the shared durable transform
  authority. Bounded JSON bindings reconstruct fixed BIT-mode application
  records, accept CHAR or UTF-8-detectable BIT input, use `DFHJSON-DATA` by
  default, enforce source conditions and SAF, and replay atomic replacements.

- Added typed CICS `TRANSFORM DATATOXML` on the shared transform runtime.
  Bounded XML bindings emit deterministic namespace/type-qualified documents,
  return paired element/type metadata with exact fullword lengths, enforce the
  source LENGERR matrix, reject illegal XML characters in data or binding
  namespaces, and replay both container bytes and metadata from the atomic
  transform ledger.

- Added typed CICS `TRANSFORM DATATOJSON` over a shared bounded transform
  runtime. Digest-pinned JSON bindings map fixed application-data fields to
  canonical JSON, named channel containers persist in BIT/CHAR modes, SAF
  protects the transformer, exact source conditions are retained, and an
  atomic transform ledger makes output replacement replay-safe across reopen.
- Added typed CICS `WRITE JOURNALNUM` as a distinct compatibility route for
  numeric journals 1–99. It maps to `DFHJnn` and shares the durable record,
  idempotency, synchronous WAIT, asynchronous REQID, SAF, and condition path
  with WRITE JOURNALNAME. Operation tag 57 is append-only; the numbered form
  reuses the reserved journal operand and output identities.

- Added typed CICS `WRITE JOURNALNAME` over the shared durable journal authority.
  The local writer preserves JTYPEID, FROM and optional PREFIX bytes with checked
  FLENGTH/PFXLENG, returns a task-owned fullword REQID for deferred output, and
  supports synchronous WAIT plus bounded NOSUSPEND/NOJBUFSP behavior. The
  versioned journal codec reads prior WAIT state, while new records retain
  idempotency identity and survive SQLite reopen. Operation tag 56, operand
  tags 99–103, and output tag 201 are append-only.

- Added typed CICS `WAIT JOURNALNUM` for compatibility with numbered journals.
  Numeric values 1–99 select `DFHJnn` in the same durable authority as named
  waits, while retaining a distinct source row, operation tag 55, and operand
  tag 98. Explicit REQID and current-buffer waits share the existing task and
  completion rules; invalid numbers are rejected before the provider route.

- Added typed CICS `WAIT JOURNALNAME` through the new generated
  `journal-control` family and one durable authority shared with the remaining
  journal commands. Explicit fullword REQID tokens are task-owned; omission
  synchronizes the journal-wide current buffer. Hardened output completes
  immediately, pending output suspends/reissues under coordinator
  cancellation/deadline control, and IOERR 17, JIDERR 43, NOTOPEN 19, and
  NOTAUTH 70 are preserved with SAF/audit and SQLite reopen coverage. Operation
  tag 54 and operand tags 96–97 are append-only; the remaining journal tag
  envelopes stay reserved and collision-tested.
- Added a checked non-LE AMODE(64) GETMAIN64 virtual-storage route. Eight-byte
  addresses use a separate allocation authority from COBOL GETMAIN; LOC24,
  LOC31, and above-bar requests occupy bounded virtual ranges. Checkpoint v11
  preserves allocation bytes, cursor generations, key and location attributes,
  and stale-address rejection. Fullword FLENGTH, NOSUSPEND, the common response
  policy, exact LENGERR/NOSTG/INVREQ paths, replay, and Memory/SQLite provider
  operation are covered. SHARED remains fail-closed pending durable cross-task
  storage: the interpreter matches allocation results to the pending request
  and rejects forged location, key, SHARED, EXECUTABLE, length, or snapshot
  attributes. No assembler source frontend or native executable storage is
  claimed.

- Added the separate non-LE AMODE(64) FREEMAIN64 DATAPOINTER and DATA routes.
  The interpreter accepts only a live eight-byte virtual allocation owned by
  the current task, including a checkpointed DATA-area binding. Release is
  tied to the pending replayed host request; stale, cross-width, and foreign
  pointers return INVREQ 16/1, while a CICS-key allocation released from a
  user-key task returns INVREQ 16/2. Checkpoint v12 preserves area bindings and
  restored stale-pointer rejection. The route uses the existing authorization,
  audit, cancellation, deadline, and Memory/SQLite replay boundary.
- Added typed CICS `SPOOLWRITE` with operation tag 62, FROM/FLENGTH operand
  tags 119–120, and LINE/PAGE option tags 79–80. It appends bounded output
  records with default LINE mode, applies the report's RECORDLENGTH and exact
  LENGERR RESP2 difference, preserves append/reply replay through SQLite
  restart, and checks SURROGAT authority for an internal-reader JOB USER card.
  The compiled route proves EIBFN `5606`.

- Added typed CICS `SPOOLREAD` with operation tag 61, MAXFLENGTH operand tag
  118, and TOFLENGTH output tag 209. A short buffer receives the bounded
  prefix, reports the truncated byte count and actual record length, and leaves
  the record pending for retry. Successful reads advance the durable cursor;
  the next read returns ENDFILE once, followed by INVREQ 16/12. Exact replay
  survives the state/outer-journal crash gap and SQLite reopen, and the
  compiled route proves EIBFN `5604`.

- Added typed CICS `SPOOLOPEN OUTPUT` with operation tag 60, NODE,
  RECORDLENGTH, and OUTDESCR operand tags 115–117, output-format option tags
  74–78, and the returned TOKEN output tag 208. The bounded provider supports
  concurrent report creation, default class A/NOCC/PRINT and 32,760-byte
  record length, double-indirect OUTDESCR parameters, exact replay and SQLite
  reopen. The compiled route proves EIBFN `5602` and both token outputs.

- Added typed CICS `SPOOLOPEN INPUT` with operation tag 59, USERID/CLASS
  operand tags 113–114, and non-ASSIGN TOKEN output tag 208. The provider
  enforces APPLID-prefix authorization, JESSPOOL SAF/audit, the JES input
  single-thread boundary with exact SPOLBUSY ownership codes, class-filtered
  deterministic selection, durable token ownership, replay, and SQLite reopen.
  The compiled route proves EIBFN `5602` and writable TOKEN delivery.

- Added typed CICS `SPOOLCLOSE` over the shared bounded durable spool state.
  Append-only operation tag 58, TOKEN operand tag 112, and KEEP/DELETE option
  tags 72–73 preserve plan compatibility. Explicit input close defaults to
  DELETE, explicit output close defaults to KEEP, ownership and `JESSPOOL`
  authorization precede mutation, and the state/replay CAS survives the exact
  crash gap before outer effect journaling. The compiled route proves EIBFN
  `5610`; report transfer and implicit-close handling remain subsequent slices.

- Added explicit local-system `SYSID` routing for typed CICS WRITEQ TD, READQ
  TD, and DELETEQ TD. Literal or storage-backed 1–4 character names must equal
  the current CICS system before authorization or mutation; unknown and
  unsupported remote systems return exact SYSIDERR 53/0 with queue state
  unchanged.

- Added typed CICS `READQ TD SET` over interpreter-owned virtual storage. SET
  is exactly alternative to INTO, returns a checked POINTER/POINTER-32 address
  to the complete consumed record, participates in checkpoint restore, and
  rejects insufficient allocation capacity without consuming the queue.

- Added typed local CICS `READQ TD QUEUE/INTO/LENGTH`. The FIFO read consumes
  exactly one durable record, uses compiler-derived INTO capacity when LENGTH
  is omitted, returns the original record length through writable halfword
  storage, and preserves IBM's consume-and-LENGERR rules for zero or truncated
  reads. Missing and empty queues return exact QIDERR 44/0 and QZERO 23/0;
  SET, SYSID, NOSUSPEND, and TDQUEUE definition modes remain fail-closed.

- Added typed CICS `WAIT EXTERNAL` over a bounded list addressed by checked
  four-byte POINTER or POINTER-32 storage. Append-only operation tag 44,
  operand tags 48–50, and option tags 28–29 preserve existing plans; NUMEVENTS,
  null/invalid list entries, first-byte `X'40'` POST-bit detection,
  PURGEABILITY, standard-versus-hand posting, selected-event completion, AEXY
  purge behavior, task cleanup, replay, and SQLite reopen are covered through
  the compiled EIBFN `5E22` route.

- Added typed CICS `WAIT EVENT` over checked four-byte POINTER or POINTER-32
  event control areas.
  Append-only operation tag 43 and operand tags 46–47 preserve existing plans;
  events without the first-byte `X'40'` POST bit suspend and reissue through the
  durable coordinator, a standard post survives SQLite reopen, completion marks
  the ECB and advances with exact EIBFN `1202`, and task cleanup removes the
  retained wait row.
- Added typed CICS `RELEASE` for row 0165 with append-only operation tag 48.
  It consumes one durable LOAD ownership level, permits a retained HOLD to be
  released by a later task, checks the program security resource before
  mutation, and records an atomic replay receipt with the ownership change.
  Its pinned condition matrix distinguishes self-release (INVREQ 16/5), an
  unloaded program (16/6), another task's non-HOLD load (16/7), RELOAD=YES
  (16/17), and an uninitialized program manager (16/30); program security
  denial returns NOTAUTH 70.
  Memory, SQLite reopen, receipt-failure recovery, denial, and compiled EIBFN
  `0E0A` regressions cover the selected route.

- Added typed CICS `LOAD` with append-only operation tag 47, operand tags
  62–65, and `HOLD` option tag 38. The compiled route returns bounded `SET`,
  `ENTRY`, `LENGTH`, or `FLENGTH` outputs from the exact immutable selected
  program generation; durable ownership survives restart and outer-receipt
  recovery, non-HOLD ownership ends with the task, HOLD ownership persists,
  and memory/SQLite plus compiled selected-route regressions cover the slice.

- Added typed CICS `INVOKE APPLICATION` with durable installed-application
  version selection, immutable program artifact/semantic identity checks,
  exact/minimum matching, bounded COMMAREA or channel identity, SAF/audit,
  restart-safe catalog reads, and compiled selected-route EIBFN `0E10` proof.
  Nested LINK dispatch now carries the exact selected artifact, program
  generation, and application content identity; the production router executes
  that artifact even when a newer generation owns the program name. Mismatch
  and multi-generation regressions plus the documented RESP/RESP2 failure matrix prevent
  program dispatch on rejected requests. For a missing current platform, the
  implementation follows the command's Conditions table (`INVREQ` 16/1), not
  the conflicting description text (`APPNOTFOUND`).

- Added typed local CICS START `TERMID` through append-only operand tag 38.
  The provider resolves active virtual terminals at command time, returns exact
  `TERMIDERR` 11/0 for an unknown identifier, persists terminal association,
  and makes RETRIEVE match that facility. At expiration, a free terminal is
  retasked through the durable coordinator; a busy terminal defers the work
  lease until available, and a deleted terminal discards the asynchronous
  request. Combined `TERMID`/`USERID`, remote/APPC facilities, and same-terminal
  multi-request coalescing remain fail-closed or pending.

- Added SQLite process-restart proof for facility-less CICS START launch. A
  claimed request can survive shutdown after promotion but before target
  creation, reclaim under a fresh lease, run its RETRIEVE target, then survive
  another shutdown before work completion without changing the target's
  durable execution journal or launching a duplicate task.

- Added automatic facility-less task launch for due local CICS START work. The
  fenced work identity now becomes the target's durable execution identity,
  the stored START principal and transaction are reauthorized at creation, and
  the started program can consume its data through RETRIEVE. A worker retry
  after target completion observes the same terminal execution and cannot
  launch a duplicate task; unavailable target definitions retain IBM's
  asynchronous no-task outcome.

- Added typed CICS `ASSIGN TNADDR` for an owned local terminal. Because the
  runtime deliberately retains no client network endpoint, the source-defined
  unresolved-address value is 39 blanks; local nonterminal use returns 16/5,
  and DPL fails closed until remote endpoint context exists. Append-only output
  tag 109 preserves existing plans.

- Added durable four-character virtual-terminal identities and typed CICS
  `ASSIGN FACILITY`/`NETNAME`. New terminal sessions allocate unique active
  identifiers such as `T000`; NETNAME follows the pinned default to that name
  padded to eight bytes. Local nonterminal requests return 16/5, FACILITY is
  DPL-prohibited at 16/200, and NETNAME without remote identity fails closed.
  Session codec `MECSB` retains strict historical reads and output tags
  107–108 are append-only.

- Added source-defined negative CICS `ASSIGN INPARTN` handling. A local
  terminal before map positioning returns `INVREQ` 16/2, local nonterminal use
  returns 16/5, and DPL returns 16/200, preserving the one- or two-byte
  receiver. Positioned-map use remains fail-closed until input-partition state
  exists; append-only output tag 106 preserves existing plan tags.

- Added typed CICS `ASSIGN INVOKINGPROG` for the local initial program. A
  trusted durable entry marker returns the source-defined eight blanks and
  keeps XCTL, LINK-child, and DPL lineage fail-closed until the runtime owns the
  caller name. Append-only output tag 105 preserves existing typed-plan tags.

- Added typed CICS `ASSIGN INPUTMSGLEN` with a bounded durable terminal-input
  length that survives RECEIVE consumption and SQLite reopen. No input returns
  halfword zero; normalized map input returns its exact byte length in local and
  DPL contexts. Its length remains in current session codec `MECSB`, which
  strictly retains `MECS1`–`MECSA` reads, and append-only output tag 104
  preserves existing plans.

- Added typed CICS `ASSIGN LANGINUSE`. The runtime's unoverridden English
  national-language default maps through the pinned CICS table to exact
  three-byte `ENU` in local and DPL contexts. Append-only output tag 103
  preserves every existing typed-plan tag.

- Added typed CICS `ASSIGN TERMPRIORITY`. The runtime's terminal definition
  uses the source default zero independently of a later `CHANGE TASK`; local
  nonterminal use returns `INVREQ` 16/5 and DPL returns 16/200. Append-only
  output tag 102 preserves every existing typed-plan tag.

- Added typed CICS `ASSIGN RETURNPROG` for a local highest-level program. It
  returns the source-defined eight blanks, refreshes the current frame's parent
  identity from each authenticated invocation, and fails closed for LINK-child
  and DPL lineage until a durable caller stack exists. Append-only output tag
  101 preserves every existing typed-plan tag.

- Added exact negative CICS `ASSIGN` handling for `DESTCOUNT`, `LDCMNEM`,
  `LDCNUM`, `PAGENUM`, and `PARTNPAGE`. Without prior BMS overflow processing,
  local requests return `INVREQ` 16/2 and preserve every receiver; DPL requests
  return the source-defined `INVREQ` 16/200. Append-only output tags 96–100
  preserve every existing typed-plan tag.

- Added typed local CICS LINK `DATALENGTH`. The value is preserved in the
  canonical request but, as IBM defines for a static local link, is not checked
  and does not shorten the LENGTH-selected COMMAREA; remote optimization and
  validation remain deferred.

- Added typed COMMAREA `LENGTH` for local CICS LINK, XCTL, and RETURN. Literal,
  numeric-storage, and matching `LENGTH OF` forms select the dispatched prefix;
  target EIBCALEN follows that prefix and unsafe ranges return bounded LENGERR.

- Added local CICS START `NOCHECK` on the typed route. An omitted REQID still
  receives a replay-stable internal row/work identity, while EIBREQID remains
  null as required; remote shipping and its reduced checking remain deferred.

- Added SQLite process-restart proof for automatic CICS DELAY wakeup. A due
  DELAY survives product/store teardown, reopens its installed program,
  terminal session, exchange, checkpoint, provider timer, and work item, then
  resumes through the ordinary worker without a client CICS resume call.

- Added in-process automatic wakeup for durable CICS DELAY work. After the
  shared worker promotes a due row, the product resolves its bounded durable
  online exchange by run-unit identity and resumes the checkpoint without a
  client CICS resume call; retry after finalized exchange cleanup is idempotent.

- Added storage-backed packed CICS DELAY `INTERVAL`. The compiler and typed
  plan now preserve numeric storage under existing operand tag 28, while
  malformed runtime packed values reach the provider's exact INVREQ response
  path. Named dynamic delays retain cancellation and checkpoint behavior.

- Added typed CICS DELAY `FOR MILLISECS` as a literal or resolved numeric
  storage value, alone or with HOURS/MINUTES/SECONDS. The shared interval value
  now retains millisecond precision, enforces pure/combined bounds with INVREQ
  RESP2 22, and returns EXPIRED for source-defined sub-50 ms delays.

- Added typed CICS DELAY packed `TIME` scheduling for integer constants and
  packed numeric storage. TIME now remains an input outside FORMATTIME, uses a
  domain-separated absolute delay identity, returns EXPIRED 31 for an elapsed
  target, and resumes through the existing durable delay worker path.

- Added typed CICS DELAY `FOR`/`UNTIL` HOURS, MINUTES, and SECONDS with
  literal or numeric-storage components. The provider uses one shared clock
  observation per new cycle, enforces conditional component ranges with INVREQ
  RESP2 4/5/6, and returns source-defined ignored-by-default EXPIRED 31 for an
  elapsed absolute target. Packed TIME and MILLISECS remain pending.

- Added local START without passed data. FROM is now optional, while LENGTH and
  FMH still require it. A no-data request schedules ordinary durable work; if
  RETRIEVE is issued for that start identity it consumes once and returns
  replay-safe ENDDATA 29/0 instead of manufacturing a zero-length success.

- Added dynamic compiled START `AFTER`/`AT` units. Append-only CICS plan tags
  retain mode and HOURS/MINUTES/SECONDS storage identities through checkpoint,
  codec, and interpreter request construction so provider-side bounds and
  response2 conditions apply to runtime values as well as literals.

- Added local START `AFTER`/`AT` scheduling with explicit HOURS, MINUTES, and
  SECONDS. The provider enforces IBM's conditional single-unit and combined-unit
  ranges with exact INVREQ 16 response2 4/5/6 and resolves one durable worker
  deadline for either relative or absolute mode.

- Added RACF-backed START USERID validity checks before surrogate authorization.
  A typed non-login principal-status request now maps unknown identities to
  USERIDERR 69/8, indeterminate/locked identities to 69/10, revoked identities
  to 69/19, and an unavailable external-security interface to INVREQ 16/18,
  with no interval or work mutation on rejection.

- Added disposition-bound crash-gap recovery for protected START. When durable
  execution terminalization precedes product/CICS cleanup, recovery rebuilds
  the exact terminal run and saved priority, commits protected rows only for
  `Completed`, rolls them back for cancelled, timed-out, failed, or dead-letter
  outcomes, and preserves handoff finalization without applying it twice.

- Added terminal disconnect and idle-timeout rollback cleanup for protected
  START. The existing caller-held terminal cleanup boundary now deletes the
  exact run's still-protected rows before releasing delay/enqueue state and
  creates no START work, without changing frozen facade sizes.

- Added implicit task-end finalization for protected START requests. Normal
  compiled completion and highest-level RETURN commit protected rows and admit
  their work; known abnormal execution completion deletes those rows without
  work. Suspension remains nonterminal.

- Added bounded START USERID surrogate admission. Typed local START accepts a
  one-to-eight-character execution identity, requires the issuing principal to
  have READ access to `SURROGAT <userid>.DFHSTART`, returns exact NOTAUTH 70/9
  before interval/work mutation on denial, and durably binds an accepted target
  principal. Omitted USERID continues to inherit the issuer; automatic target
  launch remains pending.

- Added the bounded RETRIEVE WAIT data-arrival path. Typed WAIT now checkpoints
  and reissues the RETRIEVE statement without ENDDATA or record consumption
  when no expired START data is available, then consumes normally after worker
  promotion and explicit execution re-entry. Deadlock timeout, shutdown/AICB,
  automatic wake, and process-restart proof remain pending.

- Added runtime-generated START request identities. Typed local START no longer
  requires REQID; when omitted, the provider derives a replay-stable
  eight-character uppercase identifier from the effect key and canonical
  request digest, uses it for the interval/work identity, and returns it through
  strict implicit EIBREQID state. Explicit REQID behavior is unchanged.

- Added the source-defined cancellation boundary for START PROTECT. CANCEL now
  has explicit regression coverage for NOTFND before the protected START is
  committed and normal cancellation after a committing SYNCPOINT admits its
  work. The compiled product route exercises PROTECT, SYNCPOINT, and CANCEL in
  that order while retaining the existing cancellation fence.

- Added explicit ABEND cleanup for protected START requests. Before the typed
  ABEND route transfers to an installed exit or terminates the task, it deletes
  all still-protected START rows owned by that run; no work row is created, and
  the REQID becomes reusable. Non-command abnormal termination and implicit
  task-end syncpoint behavior remain pending.

- Added the bounded local START PROTECT-to-SYNCPOINT route. PROTECT now persists
  a protected-pending START record without admitting worker work. An explicit
  successful SYNCPOINT commit releases matching records and idempotently
  creates their deterministic work; SYNCPOINT ROLLBACK durably finalizes the
  rollback, deletes those rows, and permits REQID reuse. Retry heals a committed
  record whose work enqueue was interrupted. Non-command abnormal termination,
  implicit task-end syncpoint, protected CANCEL, automatic task launch,
  PostgreSQL, and licensed evidence remain pending.

- Added RETRIEVE SET over interpreter-owned virtual storage. Typed RETRIEVE now
  accepts exactly one of INTO or a POINTER/POINTER-32 SET target with mandatory
  LENGTH. SET returns the full START record, writes its actual length, and
  installs a checked non-native pointer to task-owned bytes that survive
  checkpoint/resume. The interpreter advertises its exact remaining allocation
  capacity before the provider's consume CAS, so capacity failure leaves the
  ready record available. WAIT timeout/shutdown, terminal association,
  automatic task launch, PostgreSQL, and licensed evidence remain pending.

- Added START FMH propagation through RETRIEVE. The typed START data route now
  persists the source FMH flag in its versioned interval row; successful
  RETRIEVE returns a strict one-byte EIBFMH value (`X'FF'` with FMH, `X'00'`
  otherwise), and the interpreter owns the implicit EIB field across compiled
  checkpoint/resume. Historical retained responses without the additive output
  remain readable. Broader FMH parsing, WAIT timeout/shutdown, terminal
  association, automatic task launch, PostgreSQL, and licensed evidence remain
  pending.

- Added bounded START-to-RETRIEVE metadata propagation. Local data-bearing
  START now accepts source-checked RTRANSID, RTERMID, and QUEUE names, persists
  them in the existing versioned interval row, and RETRIEVE writes requested
  exact-width metadata outputs alongside INTO/LENGTH. Requesting metadata the
  corresponding START omitted returns ENVDEFERR without consuming the ready
  record. Compiler, plan codec, interpreter, provider, SQLite-compatible state,
  and the compiled product route share the same identities. WAIT
  timeout/shutdown, terminal association, automatic task launch, PostgreSQL,
  and licensed evidence remain pending.

- Added local application-named CICS DELAY cancellation and task cleanup.
  Positive literal INTERVAL may bind a one-to-eight-character REQID; another
  task can cancel it before expiration, the original DELAY resumes with NORMAL
  RESP2 23, and exact replay/NOTFND/expiration races remain fenced by the delay
  row and work identity. Disconnect, timeout, return, abend, and terminal
  teardown abandon outstanding delays and cancel their work. The version-two
  row codec retains version-one reads and survives SQLite reopen. Remote
  routing, TIME/units, automatic redispatch, PostgreSQL evidence, generic
  retention, and licensed differential remain pending.

- Added positive literal CICS DELAY intervals over the shared durable worker
  lane. Each task/statement cycle persists a strict versioned delay row and one
  `cics-delay-v1` work item, suspends until fenced due promotion, completes on
  reissue, survives SQLite reopen, and creates a fresh identity when a loop
  reaches the statement again. TIME and explicit units, automatic redispatch,
  PostgreSQL evidence, and licensed differential remain pending.

- Added the typed zero-delay CICS DELAY boundary. Bare/default DELAY and
  compile-time literal INTERVAL(0) now cross the typed compiler, plan,
  interpreter, canonical host request, provider, durable coordinator, and
  selected product route without creating timer state or suspending the task.
  Dynamic timing, TIME, FOR/UNTIL units, REQID on zero delay, EXPIRED, and
  automatic resumption remain pending.

- Added the typed bounded local CICS CANCEL route for unhonored committed START
  records. Explicit REQID with optional local TRANSID now authorizes the target,
  atomically tombstones the interval record, cancels queued or claimed shared
  work, returns exact NOTFND on later requests, replays the original mutation,
  and survives SQLite reopen. POST, DELAY, SYSID routing, protected-uncommitted
  START, immediate REQID reuse, and licensed differential remain pending.

- Added the versioned CICS interval START-record authority and the first typed
  local-data START-to-RETRIEVE cycle. The bounded route lowers START and
  RETRIEVE through typed plans, admits due work to the shared durable queue,
  promotes it under a fenced worker lease, consumes it once with replay-safe
  INTO/LENGTH and ENDDATA/LENGERR behavior, authorizes the target transaction,
  and survives SQLite reopen. Remote, terminal, protected, generated-REQID,
  metadata/FMH, WAIT, SET, and automatic target-task launch semantics remain
  pending, so neither command receives whole-row credit.

- Added verified offline IBM-documentation search/read commands and a cache-first
  source-review workflow for semantic development.
- Added repository contributor guidelines in `AGENTS.md` covering structure,
  development commands, coding conventions, tests, and pull request expectations.
- Added ADR-0011 and the first typed-HIR vertical slices: resolved COBOL
  arithmetic plans, typed CICS file/unit-of-work plans, versioned executable
  dialect identities, and bounded canonical plan codecs.
- Added candidate-bound product-source mutants for typed decimal receiver-local
  updates, operand capture, condition timing, rounding, and qualified
  `ADD CORRESPONDING`; unchanged exact-result tests must kill every
  representative mutant.
- Added a reviewed, pinned-source COBOL arithmetic pilot with independent
  golden bytes and typed-plan observations for `SIZE ERROR`, relative
  qualification, numeric-edited eligibility, and the no-pair no-op.
- Added persistent bounded COBOL-parser and IR-decoder fuzzing, Loom schedule
  models with a negative concurrency mutant, and instrumented critical-package
  coverage reporting as receipt-backed full CI gates.
- Added a generated documentation manifest and bounded documentation gate via
  `cargo xtask docs --check` for navigation, normative metadata, links, anchors,
  command examples, and public version truth.
- Added ADR-0008 as the current authority for the 26-package workspace
  topology, superseding ADR-0005's historical count.
- Added a docs-driven CICS file/unit-of-work conformance pilot with reviewed
  rules, exact per-obligation observations, memory/SQLite execution, restart
  faults, and a fail-closed licensed-capture adapter.
- Added digest-bound, HTML-only IBM source mapping, corpus projection, extraction,
  and implementation-independent automatic verification for all 263 CICS
  application commands in deterministic 88/88/87 batches. The accepted receipts
  contain 18,070 candidates: 16,307 objectively verified and 1,763 retained as
  bounded product ambiguity, with raw IBM publication bodies kept outside Git.
- Added a zero-credit, five-topic IBM source scope for the CICS task-enqueue
  slice. Alongside ENQ and DEQ it pins the command-level parameter page that
  establishes `DFHVALUE(TASK)=233` and `DFHVALUE(UOW)=246`, plus the ENQMODEL
  definition and global-enqueue tuning pages; publication bodies remain in the
  external content-addressed cache.
- Added typed local `EXEC CICS ENQ` and `DEQ` execution over a bounded durable
  lock catalog. The runtime preserves address-versus-content resource identity,
  nested UOW/TASK ownership, FIFO wait promotion, `NOSUSPEND`/active-handler
  `ENQBUSY`, syncpoint/task cleanup, atomic replay, and durable resume across
  memory, SQLite, and PostgreSQL store profiles.
- Added durable installed ENQMODEL definitions with bounded generic matching,
  local APPLID/SYSID isolation, nonblank-scope global serialization, disabled
  model abends, address-enqueue locality, atomic catalog installation, and
  restart validation.
- Added a zero-credit two-topic IBM source scope and typed execution for CICS
  `CHANGE TASK` and `SUSPEND`. Priority omission and `-1` remain no-ops,
  priorities `0..255` update the task and yield once, invalid values return
  `INVREQ` 16/1, and SUSPEND produces a one-shot durable scheduler handoff.
- Advertised bare ASKTIME as a distinct typed clock route that updates
  packed-decimal EIBDATE/EIBTIME without producing an ABSTIME destination.
  ASKTIME ABSTIME now uses a typed, exact `PIC S9(15) COMP-3` output binding,
  refreshes the same implicit EIB fields, and retains its eight-byte packed
  absolute-time result.
- Migrated the source-checked FORMATTIME subset to typed plans for packed
  ABSTIME input, one-byte DATESEP/TIMESEP values, fixed YYYYMMDD/YYMMDD/MMDDYY/
  MMDDYYYY/YYDDD/TIME fields, and fullword MILLISECONDS output. Negative or
  malformed absolute time returns INVREQ 16/1, while the remaining official
  formats stay explicitly deferred.
- Migrated `EXEC CICS ABEND` from raw compatibility lowering to a typed task
  plan with an optional 1–4 character ABCODE and explicit CANCEL/NODUMP flags.
  Typed execution preserves HANDLE ABEND transfer, task-enqueue cleanup,
  terminal dump disposition, EIBFN, audit, and durable replay behavior without
  retaining command source text.
- Migrated `HANDLE ABEND` to a typed task plan with distinct LABEL and PROGRAM
  operands plus CANCEL/default-cancel and RESET actions. Compiler, plan-codec,
  interpreter, provider, and selected-route checks preserve the bounded action
  alternatives, program authorization, and durable active/canceled exit state.
- Migrated the local `LINK PROGRAM ... COMMAREA` compatibility subset to a
  typed program-control plan. PROGRAM is a bounded literal or resolved field,
  COMMAREA is one identity-checked input/output slot, and the adapter preserves
  its captured bytes when registering the return binding. A compiled selected
  route verifies authorization, EIBFN `0E02`, returned bytes, audit, and durable
  suspension; remote/channel/length forms remain deferred.
- Migrated the local `XCTL PROGRAM ... COMMAREA` compatibility subset to a
  typed program-control plan. Its COMMAREA is input-only, the provider returns
  an unconditional frame-replacing transfer, and the selected online route
  initializes the target program's `DFHCOMMAREA` while preserving EIBFN `0E04`.
  Channel, input-message, explicit-length, DPL, and licensed forms remain
  deferred.
- Migrated bare `EXEC CICS RETURN` and the local TRANSID/COMMAREA
  pseudo-conversation subset to a typed task-control plan. TRANSID is a
  prevalidated 1–4 character literal or field, COMMAREA is captured as an
  input-only copy, and the durable selected route completes the old execution
  before the next terminal task. `LENGTH(LENGTH OF commarea)` selects the
  captured prefix; other lengths, channel, input-message, immediate, BTS,
  higher-level, and DPL ownership remain deferred.
- Migrated the default-cursor `STARTBR`/`READNEXT`/`READPREV`/`ENDBR` file
  browse subset to typed plans. The compiler binds exactly one FILE/DATASET
  resource and a writable RIDFLD, admits STARTBR's default-equivalent `GTEQ`
  and exact-key `EQUAL` relations as an exclusive choice, models RIDFLD as the
  same input/output slot on reads, and writes the returned record into a
  pre-resolved INTO area. STARTBR now also accepts bounded KEYLENGTH forms,
  requires KEYLENGTH for GENERIC, preserves prefix-only EQUAL positioning,
  supports GENERIC GTEQ and KEYLENGTH zero first-record positioning, and maps
  definition/full-key violations to exact INVREQ 16/25, 16/26, or 16/42.
  READNEXT and READPREV LENGTH now use one writable halfword as input capacity
  and actual-length output, truncate oversized records with LENGERR 22/11,
  reject omitted variable-record lengths with 22/10, and preserve fixed-record
  mismatch 22/13.
  REQID/SYSID, alternate key modes, SET, and UPDATE/TOKEN/RLS semantics remain
  fail-closed.
- Migrated the explicit-key `DELETE` and `WRITE FILE` compatibility subsets to
  typed file-mutation plans. RIDFLD and WRITE FROM are resolved data-area
  inputs, FILE/DATASET remains one exact resource alias, and both operations
  carry typed mutation identity. DELETE may instead consume the latest record
  held by `READ UPDATE`. READ LENGTH is a writable halfword capacity that
  returns the actual record length. WRITE and REWRITE LENGTH accept a bounded
  literal, halfword-binary value, or matching `LENGTH OF` and persist only that
  prefix;
  READ, WRITE, and explicit-key DELETE KEYLENGTH accept the same positive
  halfword forms against RIDFLD and preserve exact `INVREQ` 16/26 for a
  definition mismatch. TOKEN deletes, remote forms, generic and alternate
  record identifiers, mass insert, and RLS wait controls remain deferred. READ
  GTEQ uses the existing bounded dataset cursor primitive to return the equal
  key or first greater keyed record without retaining browse state; READ
  GENERIC uses a positive partial KEYLENGTH and returns only a matching prefix.
  Explicit READ EQUAL now selects the same source-defined exact complete- or
  generic-key behavior as the default relation and conflicts with GTEQ.
  READ GTEQ also accepts source-defined KEYLENGTH zero to select the first keyed
  record, including with GENERIC, while zero remains rejected for default EQUAL
  and GENERIC without GTEQ.
- Repaired the CICS file/UOW conformance pilot after typed REWRITE began
  requiring a storage-backed FROM area. Mutation fixtures now move their exact
  bytes into the declared record area before REWRITE, and the WRITE LENGTH
  source is test-only so the pilot module again meets its reviewed production
  line ceiling and warnings-denied build.
- Restored the CICS semantic-family module policy after interval, task-start,
  and storage handlers were added. Deterministic interval normalization now
  lives under the reviewed handler tree with stable public re-exports, and the
  frozen module inventory enumerates every current semantic handler module.
- Repaired the typed-semantic architecture guard after legacy CICS execution
  moved behind a module re-export. The guard now bounds typed execution at the
  first legacy helper and distinguishes a forbidden bare grammar `arguments`
  call from the typed SET allocation helper.
- Removed an application-specific profile name from the production dataset
  replay-index documentation; the optimization and replay behavior remain
  generic and unchanged.
- Updated the durable-retention architecture guard for the memory store's
  instrumented `snapshot` staging API. It continues to require staged archive
  insertion and one final atomic state replacement without depending on the
  retired direct-clone spelling.
- Updated the CICS command-descriptor schema from seven to the exact nine
  reviewed runtime families after interval-control and storage-control were
  frozen into the generator authority.
- Updated the generated CICS application-contract schema to the current
  readiness split: 41 typed, 0 legacy, 41 advertised, and 222 unready rows,
  with an exact 24-family summary bound.
- Reconciled the frozen CICS source-map view with newer typed runtime
  admissions. DELETEQ TD, FREEMAIN, and GETMAIN are excluded from the
  no-admission compatibility projection, and all three maps now bind the
  current descriptor digest.
- Propagated the regenerated CICS map identities into the three zero-credit
  source corpora and their independently recomputed corpus digests. No topic,
  retained HTML, source fact, or browser receipt changed.
- Propagated the refreshed map/corpus identities through all three CICS
  extraction plans and recomputed their canonical plan digests without
  changing selectors, bounds, resolutions, or expected projection shapes.
- Propagated the refreshed map, corpus, and extraction-plan identities through
  all three zero-credit CICS candidate projections and recomputed their
  canonical projection digests without changing any of the 18,070 candidate
  facts or their review states.
- Rebound the three accepted CICS source-review envelopes and generated
  application-command contract to those refreshed candidate files. The prior
  independent-verification report identities, all dispositions, readiness
  states, and command facts remain unchanged; no source replay or credit was
  claimed. Descriptor test ratchets now reflect the already sealed 41 API
  operations, including DELETEQ TD, FREEMAIN, and GETMAIN.
- Migrated the local `WRITEQ TD` compatibility subset to a typed queue-write
  plan. QUEUE is a validated 1–4 character literal or field, FROM is a resolved
  data area, and optional numeric LENGTH or `LENGTH OF` selects the persisted
  prefix before idempotency comparison. Remote SYSID routing and unimplemented
  TDQUEUE definition/open/disabled condition semantics remain fail-closed.
- Added the typed local `DELETEQ TD` subset. QUEUE uses the same validated
  selector and RACF queue resource as `WRITEQ TD`; successful execution
  atomically removes the durable queue and releases its retained-byte count,
  while a missing queue returns exact `QIDERR` 44/0. Memory, three-open SQLite,
  and selected compiled-route regressions cover deletion and repeated-delete
  behavior. Remote SYSID and TDQUEUE definition, extrapartition, disabled, and
  locked states remain fail-closed.
- Added typed local `DELETEQ TS` QUEUE and QNAME forms through append-only
  operation tag 41 and long-name operand tag 43. QUEUE accepts 1–8 characters;
  QNAME accepts 1–16 and preserves the source-required 16-byte field. The
  runtime authorizes the selected resource, validates the durable `cics-tsq`
  row before versioned deletion, frees all stored items, and returns exact
  `QIDERR` 44/0 or all-zero-name `INVREQ` 16/0 through ordinary condition
  handling. An explicit SYSID matching the local system uses the same path;
  an unknown system returns `SYSIDERR` 53/0 without mutation. Replay and SQLite
  reopen preserve a single deletion; remote/shared dispatch, TSMODEL state,
  locking, and WRITEQ/READQ TS remain pending.
- Added typed task-local `GETMAIN SET` with exactly one FLENGTH or compatibility
  LENGTH over interpreter-owned virtual storage. FLENGTH accepts signed
  fullword input; LENGTH accepts unsigned halfword input and enforces its 65,520
  byte ceiling. One-byte `INITIMG`, `NOSUSPEND`, checkpoint restoration, replay,
  LENGERR 22/1 pointer clearing, and default-ignored NOSTG 42/2 are covered
  without exposing native addresses. Key/share/executable policy, 64-bit forms,
  and DPL proof remain fail-closed or pending.
- Added typed task-local `FREEMAIN DATAPOINTER` and `FREEMAIN DATA`. The
  interpreter validates either the pointer value or the DATA area's current
  virtual-storage view against a live, offset-zero GETMAIN allocation, applies
  a replay-bound release, checkpoints stale identity, rejects repeat, static,
  or foreign releases with exact `INVREQ` 16/1, and restores released
  frame/byte capacity. Key/shared/load ownership, FREEMAIN64, and DPL proof
  remain pending.
- Migrated bounded local `RECEIVE MAP`, `SEND MAP`, and `SEND TEXT` subsets to
  typed terminal plans. MAP and optional MAPSET are validated 1–7 character
  selectors, with an eight-byte RECEIVE MAPSET field admitted for a valid name
  plus trailing blank. MAPSET defaults to MAP, and FROM/INTO storage is
  resolved before dispatch. `SEND MAP LENGTH` and `SEND TEXT LENGTH` accept a
  literal, halfword binary value, or matching `LENGTH OF` form and select only
  that prefix of an explicit FROM area; out-of-range SEND TEXT values return
  exact `LENGERR` 22/0 before screen mutation. `SEND MAP MAPONLY` rejects
  application FROM/LENGTH data and writes only the initialized defaults from
  the selected map. `SEND MAP DATAONLY` requires explicit symbolic FROM data,
  ignores map defaults, applies supplied field attributes, and preserves the
  prior field attribute when the supplied byte is `X'00'`. RECEIVE uses the
  requested durable definition for terminal-fit and field normalization.
  `RECEIVE MAP FROM` accepts an optional literal, halfword-binary, or matching
  `LENGTH OF` value, maps only that supplied prefix, and leaves queued terminal
  input untouched. Explicit `RECEIVE MAP TERMINAL` selects the originating
  terminal path, rejects combination with FROM before state mutation, and
  carries a typed empty option through the compiled selected route. SET
  pointers, TIOAPFX handling, omitted-map AID-only receive, implicit symbolic
  map storage, paging, translation, partition, and other device controls remain
  fail-closed.
- Updated the locked `rustls` dependency from 0.23.43 to 0.23.45 so the
  release dependency gate is not exposed to `RUSTSEC-2026-0285`.
- Added a typed `PURGE MESSAGE` route for the runtime's reachable empty
  full-BMS logical-message state. Local execution is an idempotent audited
  mutation that preserves the displayed terminal image; DPL execution returns
  the source-defined `INVREQ` 16/200. Accumulated pages and `TSIOERR` remain
  fail-closed until a full-BMS ACCUM/page authority exists.
- Migrated the 78 source-bounded `ASSIGN` context outputs from raw command-text
  compatibility lowering to one typed task plan. Each output carries an
  append-only semantic name and pre-resolved writable storage identity through
  publication and defensive admission, while the existing 16-option, receiver,
  partial-INVREQ, DPL, EIBFN, provider and retained-artifact behavior is
  preserved.
- Expanded the source-bounded CICS `ASSIGN` route with APPLID,
  SYSID, USERID, TASKPRIORITY, and the exact absent application/channel context
  defaults. It also reports absent CWA/TWA lengths, null OPERKEYS, and a normal
  task start without inventing state. The compiler enforces the 16-option limit
  and halfword/fullword/exact-width receivers, the provider rejects unknown or
  wrongly typed arguments, and local OPSECURITY/TCTUALENG defaults become exact
  DPL `INVREQ` 16/200 failures while unrelated requested outputs still populate.
  With no configured initialization parameter, INITPARM remains unchanged and
  INITPARMLEN returns halfword zero. PROGRAM is derived from the trusted current
  execution frame and follows durable HANDLE ABEND program transfers. Compiled
  local tasks with no pending next transaction receive four blanks from
  NEXTTRANSID, while DPL use returns `INVREQ` 16/200. BRIDGE returns four
  blanks because bridge-started tasks are outside the runtime. Compiled online
  tasks also receive exact zero ABOFFSET, instruction-interrupt, PSW, and
  register diagnostics because no recoverable ASRA-class machine-check handoff
  exists. They observe these values, the durable terminal's
  current/default/alternate screen geometry, and priority changes through the
  selected route. With no transaction-abend-control-block message, ERRORMSG
  and ERRORMSGLEN return 500 null bytes and halfword zero. Screen options fail
  with
  `INVREQ` 16/5 for nonterminal tasks and `INVREQ` 16/200 in DPL; FCI
  distinguishes the supported terminal facility (`X'01'`) from no facility
  (`X'00'`) and is also DPL-prohibited. LINKLEVEL returns one for a top-level
  local program and two for a DPL target behind its level-one mirror; unmodeled
  deeper local stacks fail closed. With no application partition set, PARTNSET
  returns six blanks on a terminal task and follows the local/DPL `INVREQ`
  matrix. MAPCOLUMN, MAPHEIGHT, MAPLINE, and MAPWIDTH resolve the `MECM6`
  durable definition of the most recently sent map; historical `MECM1`–`MECM5`
  definitions retain their prior top-left origin. Numeric DFHMDI LINE/COLUMN
  now offset TN3270 fields, maps that exceed the terminal receive source-named
  `INVMPSZ` 38, absent maps receive `INVREQ` 16/2, and DPL returns 16/200. The
  fixed CP037 region encoding is exposed as fullword LOCALCCSID 37 in both
  local and DPL execution. The same owned virtual terminal reports a
  3270 data stream and no basic SCS data stream; its unsupported optional
  device capabilities return false indicators, and its interactive session
  profile returns the attended indicator. CMDSEC and RESSEC return `X` because
  command admission and resource-owning operations use the platform's mandatory
  authorization routes.
  QNAME fails with exact `INVREQ` 16/4 because no task can be started by an ATI
  trigger, and with 16/200 in DPL. ACTIVITY, ACTIVITYID, PROCESS, and
  PROCESSTYPE fail with exact `INVREQ` 16/6 because no BTS activity path exists.
  DESTID and DESTIDLENG similarly report 16/3 before any BDI command and
  16/200 in DPL. PRINSYSID reports 16/5 because the runtime has no MRO, LU6.1,
  or APPC principal facility, including in a DPL server program.
- Added online continuation format `MEOM4`, which retains the current task
  priority and an optional staged program-transfer handoff. Readers preserve
  `MEOM3` priority rows and historical `MEOM2` rows without that field.
- Added a zero-credit IBM source scope and typed CICS `SET ASSOCIATION
  USERCORRDATA`. The task-owned value overwrites with IBM's silent 64-byte
  truncation, enforces originating-task and command-security checks, and uses
  replay-bound `MECS5` session rows with `MECS1`–`MECS4` read compatibility.
- Added typed CICS `ADDRESS SET` for both documented COBOL directions. The
  compiler distinguishes pointer references from `ADDRESS OF` data areas, the
  provider validates only opaque storage identities, and the interpreter
  applies checked virtual aliases after successful audited dispatch.
- Added typed CICS `ADDRESS COMMAREA` through append-only operation tag 42 and
  pointer-target operand tag 45. A four-byte POINTER/POINTER-32 receives a
  checked virtual address for the current program's DFHCOMMAREA; an absent or
  unassigned COMMAREA receives exact `X'FF000000'`. The address can be consumed
  by the existing ADDRESS SET route without exposing a native process address;
  ACEE, CWA, EIB, TCTUA, and TWA remain pending.
- Added source-backed CICS `ABEND NODUMP` admission and explicit terminal dump
  disposition. Valid nonreserved ABCODE values request a dump, omitted or
  invalid codes and NODUMP suppress it, and retained older outcomes remain
  distinguishable as having no recorded dump decision.
- Added single-level CICS `HANDLE ABEND RESET` lifecycle semantics. Dispatching
  an active label automatically deactivates it, RESET reactivates the canceled
  label, bare HANDLE ABEND defaults to CANCEL, and conflicting action forms
  fail before execution.
- Added typed CICS `PUSH HANDLE` and `POP HANDLE` over a bounded 64-frame task
  stack. Nested frames suspend and restore condition/ABEND specifications,
  unmatched POP returns exact INVREQ behavior, and a compiled online route
  proves inner-to-outer exit restoration.
- Added typed CICS `IGNORE CONDITION` for 1–16 unique generated EIBRESP names.
  Ignored failures continue with the EIB set, HANDLE CONDITION overrides the
  matching ignore, PUSH/POP preserves it, and hostile lists fail before task
  state changes.
- Promoted CICS `HANDLE CONDITION` to typed execution for 1–16 generated
  EIBRESP names. One command atomically installs or deactivates every selected
  handler, specific actions precede generalized `ERROR`, and canonical or
  legacy duplicates and malformed labels fail before task state changes.
- Pinned a zero-credit CICS `HANDLE AID` source scope containing the exact
  command page and its linked BMS/DFHAID constant authority, reproduced through
  the existing Chrome session. This source receipt grants no execution or
  licensed differential credit.
- Added typed CICS `HANDLE AID` for the 34 source-named terminal AIDs, including
  optional-label deactivation, exact-over-`ANYKEY` precedence, the complete
  reached DFHAID byte set, PUSH/POP participation, and DPL `INVREQ` 16/200.
- Added durable `MECS7` CICS HANDLE state. Condition, AID, IGNORE, typed
  LABEL/PROGRAM ABEND exits, and nested PUSH/POP specifications now use session
  CAS, roll back on failed persistence, survive a terminal-input handoff and
  SQLite reopen, and clear when the task completes or recovery discards a
  non-handoff terminal task. `MECS6` label-only state remains readable.
- Extended the session state to `MECS9` with the first and latest explicit EXEC
  CICS ABEND codes, dump request, and failing program. ASSIGN ABCODE, ORGABCODE,
  ABDUMP, and ABPROGRAM now survive repeated-handler and program-transfer
  handoffs and SQLite reopen; `MECS8` remains readable by treating its sole
  code as both original and current, and `MECS7` retains no abend history.
- Added current-level CICS `HANDLE ABEND PROGRAM(name)` with exact local-program
  SAF and PGMIDERR checks, issuing-program COMMAREA transfer, CANCEL/RESET and
  PUSH/POP participation, a compiled two-program selected route, and recoverable
  artifact-bound execution handoff. Outward LINK-level search remains pending.
- Added the frozen-with-bounded-ambiguities CIC-901 command contract and generated
  263-row compiler registry. The registry explicitly separates three typed runtime
  handlers, 20 legacy compatibility handlers, and 240 unready handlers; automatic
  registration remains disabled and no default handler exists. The contract
  binds one 121-name EIBRESP condition authority and truthfully classifies the
  participant boundary as two known mutating rows, 260 bounded-effect rows, one
  explicit UOW boundary, and 261 bounded-UOW rows.
- Added candidate-aware EXEC CICS compiler recognition and fail-closed validation
  for source-reviewed command heads, COBOL applicability, option value shapes,
  discriminators, dependencies, alternatives, exclusions, and known source bounds.
  This seals only the non-release CIC-901 implementation boundary; it grants no new
  execution, coverage, or differential credit to unready commands.
- Added the first CIC-902 recovery guard: an owned execution-context binding
  rejects DPL `SYNCPOINT` without `SYNCONRETURN` or under `DPLSUBSET` with exact
  `INVREQ` RESP/RESP2 before unit-of-work mutation.
- Added a bounded remote-syncpoint outcome binding: a `SYNCONRETURN` DPL commit
  that the remote system cannot commit now rolls back local recoverable work,
  durably finalizes the rolled-back UOW, and returns exact `ROLLEDBACK` RESP 82
  with replay-safe behavior. A zero-credit selected-route regression drives the
  condition through typed COBOL, Conformance IR, the coordinator, and product
  providers.
- Implemented the source-defined `ABEND CANCEL` behavior on the existing task
  path: it cancels the active HANDLE ABEND exit before abnormal termination,
  persists no stale target, and is now admitted by the generated legacy option
  catalog. Dump disposition and typed task-control migration remain pending.
- Added a reviewed COBOL numeric `MOVE` pilot and corrected floating-insertion,
  capacity, sign, and overflow behavior found by that review.
- Added cost-aware local Jenkins assurance, exact-candidate command receipts,
  PostgreSQL parity helpers, and bounded dataset mutation checks.

### Changed

- Extended typed CICS file control with lossless `READ LENGTH`/`KEYLENGTH` and
  `REWRITE LENGTH` operands, including `LENGTH OF`, actual-length reporting,
  full-key validation, truncation, and sourced `LENGERR`/`INVREQ` responses.
  The behavior is bound to baseline
  `ibm-cics-ts-6x-file-uow-pilot-2026-09-08`, catalog rows
  `ibm-cics-ts-6x-2026-08-31:api-commands:0156` and `:0181`, and the Options
  and Conditions sections of
  `SSJL4D_6.x/reference-applications/commands-api/dfhp4_read.html` and
  `SSJL4D_6.x/reference-applications/commands-api/dfhp4_rewrite.html`.
- Scoped local CI to the last successful ancestor, preserving policy/docs
  checks while skipping runtime rebuild/deployment for prose-only changes. Added
  per-command timings, bounded stage timeouts, and focused agent verification rules.
- Organized shared Git ignore rules for generated output, runtime state,
  environment secrets, and local tooling while retaining templates and evidence.
- Advanced decimal assignment to a policy-bearing `@2` contract with explicit
  arithmetic context, COBOL numeric-storage ABI, receiver-update, condition,
  and rounding behavior. The exact `@1` compatibility route remains readable,
  while an independent `ledger.formula@1` adapter proves bounded reuse without
  importing COBOL HIR.
- Made `mainframe.core.cobol@1.define` a dialect-owned semantic contract so
  legalization, artifact admission, and defensive VM admission reject malformed
  static layout ABI metadata before runtime construction.
- Corrected omitted-minimum ODO parsing, clause boundaries and phrase ordering,
  qualified ODO execution, and bounded key/index admission against the pinned
  grammar authority. Unsupported DYNAMIC table/alias combinations and TYPEDEF
  ODO objects now fail before publication; CONDITION and RENAMES associations
  no longer inherit REDEFINES-only constraints. Executable admission also checks
  nonnumeric category shapes, physical alias topology, RENAMES endpoints, and
  the bounded level-88 value subset shared with the frontend.
- Advanced artifact publication to `mainframe-env.artifact@3`; manifests now
  carry the exact dialect namespace/major set derived from their executable
  payload and bind COBOL arithmetic/display-sign/LP options to payload config,
  while historical `@2` artifacts retain their original bytes and contract.
- Made the release-smoke rejection test cover development hosts that are not
  advertised release targets, including native Linux ARM environments.
- Updated the 0.9 CICS implementation plan to require the integrated 28-finding
  hardening baseline, bounded family slices, per-slice security/recovery,
  explicit backend validation, and early licensed-campaign planning.
- Moved the 20 raw CICS compatibility routes' executable option subsets into a
  versioned runtime-admission catalog and the generated application registry,
  removing a handwritten compiler allowlist and rejecting catalog/runtime
  drift during generation. The move also removed the unreachable `ASSIGN
  TRANSID` entry, which is absent from the pinned application-command syntax.
- Normalized the verified ENQ/DEQ syntax so direct `UOW` and `TASK` lifetime
  forms remain distinct flags from `MAXLIFETIME(cvda)`. The generated registry
  now requires `RESOURCE`, resolves RESOURCE/MAXLIFETIME as inputs, and enforces
  the three lifetime spellings as mutually exclusive without advertising the
  still-unimplemented commands.
- CICS result handling now distinguishes a source-defined ignored condition
  from normal completion, updates `EIBFN` from the generated application row,
  and retains an ENQ suspension as the same durable online task until dequeue,
  timeout, or cancellation cleanup.
- Added a blocking `missing_docs` ratchet for every contract crate, reduced the
  initial execution/store debt, and added runnable lifecycle/store examples.
- Split Db2, IMS, and MQ durable state into independently versioned object,
  index, cursor, unit-of-work, and replay rows with atomic legacy migration.
- Assigned post-0.8.2 work the distinct `0.8.3` development identity and made
  released versus development state explicit in every version authority.
- Replaced the live GitHub Actions assurance path with the capped local Jenkins
  workflow; hosted metadata remains historical rather than current evidence.
- Moved official catalog extraction to pinned IBM topic markup and strengthened
  locator, publication-byte, generated-registry, and source-review guards.
- Versioned canonical host-effect digests and tightened installed-call replay,
  live cancellation, deadline, provider-move, and DCOLLECT hardening after the
  0.8.2 tag.

### Fixed

- Corrected typed CICS `WAIT EVENT` and `WAIT EXTERNAL` to test only the
  first-byte `X'40'` ECB POST bit instead of treating any nonzero fullword as
  posted, and accept POINTER and POINTER-32 for their ptr-value operands only
  when the resolved pointer storage is exactly four bytes.

- Apply typed CICS `WRITE LENGTH`/`KEYLENGTH` through persisted records, resolve
  dynamic legacy `SEND LENGTH` operands, preserve typed `SEND MAP` ownership
  during compatibility probing, and keep FORMATTIME admission aligned with its
  documented output-field widths. Level-88 hexadecimal values now use the same
  alphanumeric space-padding comparison as quoted values, and exhausted
  internal-reader child admission cancels the unadmitted child instead of
  stranding it queued without work.

- Refresh CardDemo resource, base-batch, IMS and full derived receipts for existing
  main behavior after reproducing release 0.1.1 and comparing record bytes; retain
  historical CD-023 bytes through the existing 0.8 receipt, and keep all workload
  and source-contract assertions (#219).

- Refresh CD-024's derived spool digests for the existing JES-803 step-scoped
  output layout, preserving record bytes and workload checks (#218).

- Retain inline Db2 cursor declarations on OPEN so FETCH and CLOSE can authorize
  the original table, including after restart (#217).

- Accept underscores in RACF service resource profiles, consistent with the
  existing security model and command interface (#216).

- Provision CardDemo DB2 maintenance table permissions for WEBADM while retaining
  IBMUSER installation access and denying WEBUSER; permit the readiness probe
  to read SYSIBM.SYSDUMMY1 without granting writes (#215).

- Fixed typed `RETURN` lowering and execution for `LENGTH(LENGTH OF
  commarea)` (#202).
- Fixed typed `SEND MAP`/`SEND TEXT` admission for the reached `ERASE`,
  `CURSOR`, and `FREEKB` flags (#203).
- Fixed typed `STARTBR` admission for its default-equivalent `GTEQ` option
  (#204).
- Fixed current-record `DELETE` after `READ UPDATE`, including `INVREQ` 16/31
  when no record is held (#205).
- Fixed typed plan validation accepting unrelated extension flags on READ, REWRITE,
  and SYNCPOINT during the PR #179 merge (#212).
- Fixed online XCTL and program-exit transfers dropping prior CICS trace entries
  when replacing the volatile run, introduced by `ad53b3f` (#213).
- Fixed CardDemo DB2 control-library allocation to reserve directory space for all
  members (#201). CNTL seeding from `3d1a55b` exceeded the directory capacity
  enforced by `fe2c1ee`; seven control members now receive two directory blocks.
- Fixed typed `WRITEQ TD` lowering for `LENGTH(LENGTH OF data-area)` (#206).
- Fixed bare `DATESEP`/`TIMESEP` defaults and compact FORMATTIME output widths
  (#207).
- Fixed typed `RECEIVE MAP` lowering and runtime trimming for eight-byte
  `MAPSET` data areas (#208).
- Fixed the stale licensed COBOL oracle digest used by `spec --check` (#209).
- Fixed `SystemClockProvider`'s request and result budgets being too small for
  the canonical encoding the host-call guard measures, so CardDemo bill
  payment's `EXEC CICS ASKTIME` failed with `ResourceExhausted`
  (`toreleon/mainframe-env#195`). `bf749b2` switched
  `ScopedHostService::invoke` to measure `canonical_request_size` and
  `canonical_result_size` instead of `Debug`-formatted length, but didn't
  re-budget the clock provider's `4309a0a`-era 64-byte `max_request_bytes`
  and `max_result_bytes` (`crates/apps/mainframe-env-server/src/product.rs`,
  `SystemClockProvider::new`), so every nested `Clock` request (135/127/127
  canonical bytes for `UtcTimestamp`/`Date`/`Time`) was rejected before the
  provider ever dispatched. Both budgets are now 256 bytes, sized to the
  measured canonical request and result sizes of every `ClockRequest`
  variant. `cargo xtask carddemo-base-online --check` now passes.
- Fixed `DatasetService` reloading and fully decoding the whole `dataset-replay`
  provider-state index twice on every dataset request, regardless of whether
  any row had changed (`toreleon/mainframe-env#194`, introduced by `a5fbc43`).
  `DatasetService::invoke_checked`
  (`crates/providers/mainframe-env-dataset/src/service.rs`) called
  `refresh_replay_index()` and then, after taking the state lock, reloaded the
  index a second time; both reloads decoded and validated every listed row,
  about 43 µs per row, so cost grew with every replay row ever persisted. A
  new `ReplayIndex` (`crates/providers/mainframe-env-dataset/src/replay_index.rs`)
  syncs the index with one `list_provider_state` call per request,
  re-decoding a row only when it changed and dropping keys no longer listed;
  a corrupt or duplicate row still fails the request closed without
  partially applying the sync. `invoke_checked` now syncs once, under the
  state lock, before `HostRequest::validate`, so store and corruption
  errors still precede `Malformed`. `refresh_replay_index` is now
  sync-plus-`len`.

  The first cut of this fix kept a `(version, payload SHA-256)` fingerprint
  per key and rebuilt the index map on every sync, which removed the
  repeated decode but still cost about 13.4 µs per replay row on every
  request, even when nothing had changed: an instrumented
  `carddemo-operator-submit` gate run (logging every 25th
  `invoke_checked`'s phase timings) found about 10 of those 13.4 µs/row
  recomputing the SHA-256 digest and about 2.8 µs/row cloning unchanged
  entries into a freshly rebuilt `BTreeMap`; request application itself grew
  only about 0.4 µs/row. `ReplayIndex` now keeps the committed payload bytes
  in each entry and detects a changed row by comparing `version`, then
  payload length, then payload bytes — no digest — and updates
  `self.entries` in place instead of rebuilding it, so an unchanged sync
  touches, clones, or decodes nothing. A request still lists the namespace
  and compares each row's bytes, so some per-row work remains, but it is
  small enough that the CREASTMT (STEP040, `CBSTM03A`) slowdown in
  `toreleon/mainframe-env#185` is gone: `timeout --signal=KILL 590 cargo
  xtask carddemo-operator-submit --check` now completes and passes, in about
  3 minutes 21 seconds, where it was previously killed at 590 s without
  finishing.
- Fixed `STARTBR` rejecting a full-length all-`X'FF'` `RIDFLD` under the
  default `GTEQ` relation with `NOTFND` instead of positioning the browse at
  the end of the data set for `READPREV` (IBM topic
  `SSJL4D_6.x/reference-applications/commands-api/dfhp4_startbr.html`, RIDFLD
  option). COTRN02C's add-transaction browse
  (`MOVE HIGH-VALUES TO TRAN-ID` then `STARTBR ... KEYLENGTH(LENGTH OF
  TRAN-ID)`) relies on this VSAM behavior to find the last transaction ID, so
  the online add journey (`toreleon/mainframe-env#191`,
  `carddemo.online.transaction_add_drift`) wrote no record. `0df4cdb` added
  the `relation` field and a bounds check to `DatasetRequest::StartBrowse`
  (`crates/providers/mainframe-env-dataset/src/service.rs`) that rejected any
  out-of-range key without registering a cursor. The CardDemo journey only
  reached it once #181, #183, #184 and #187 were fixed. A full-length
  all-`X'FF'` key under `GreaterOrEqual` on a non-empty data set now
  registers the cursor at `identities.len()`, ready for `READPREV`; a
  shorter `GENERIC` all-`X'FF'` key, a non-`X'FF'` out-of-range key, and any
  key on an empty data set keep returning `NOTFND`.
- Removed one of two causes of `CREASTMT` (STEP040, `CBSTM03A`) slowing down
  over its run and never finishing in the `carddemo-operator-submit` gate;
  the other, the dataset replay index, is the `#194` entry above.
  `MemoryStore`'s per-effect journal methods (`admit_execution`,
  `commit_execution_step`, and `mutate_provider_states_atomic`, which backs
  `put_provider_states_atomic`, in
  `crates/stores/mainframe-env-store/src/memory.rs`) staged every write by
  cloning the whole `State`, a cost proportional to store size. `c77006a` (#55)
  made the installed-program child `CALL` path durable instead of going
  through `ExecutionCoordinator::with_host`, so each nested `CALL 'CBSTM03B'`
  journals its own execution and made about 9 of these clones instead of
  about 2. These methods now mutate the locked `State` in place under an undo
  log (`memory/journal.rs`) that records the prior value of only the entries a
  call touches and restores them, in reverse order, on any `Err`; the six
  cold-path clones (retention, archive, reconcile) are unchanged. After this
  change sequential `TRNXFILE` calls stayed flat at about 108 ms each; a
  profile then showed that remaining time was the dataset replay index
  being decoded on every request (`toreleon/mainframe-env#194`), which is
  fixed separately.
- Fixed the CardDemo card-list selection (`COCRDLIC`, menu COMEN01 option 3)
  ignoring a row picked with `S`/`U`: choosing a card redisplayed the list
  (mapset `COCRDLI`) with `ERRMSG` `INVALID ACTION CODE` instead of `XCTL`ing
  to `COCRDUPC`. `2250-EDIT-ARRAY`'s subscripted level-88 `SELECT-BLANK`
  (`WS-EDIT-SELECT(n)`, `88 SELECT-BLANK VALUES ' ', LOW-VALUES`) never
  matched a blank row's single space byte: `condition_matches`
  (`crates/kernel/mainframe-env-interpreter/src/machine/
  condition_literals.rs`) compared the field's `.trim()`-med text against the
  `' '` literal's quote-trimmed (but not space-trimmed) text, so a field
  holding exactly one space compared `""` against `" "` and never matched.
  Every blank row then fell into `EVALUATE TRUE`'s `WHEN OTHER`, which set
  `INPUT-ERROR`/`WS-INVALID-ACTION-CODE` and masked the correctly selected
  row's `I-SELECTED`. The trimmed comparison dates from `da74f19`, before
  0.1.1; the change that made the `' '` value reach it with its space intact
  after 0.1.1 is not pinned. `condition_matches` now compares the field's raw
  bytes against the literal's raw bytes after space-padding the shorter
  operand to the longer's length, matching IBM Enterprise COBOL 6.5
  alphanumeric comparison rules, so a space-only literal is no longer
  indistinguishable from an empty one. `toreleon/mainframe-env#187`.
- Fixed the CardDemo card list (`COCRDLIC` menu COMEN01 option 3) failing
  `9000-READ-FORWARD-EXIT`'s unconditional `ENDBR` with an unhandled
  `INVREQ`, a regression from `0204c9b` on this branch. `0204c9b` made
  `file_control.rs`'s `decimal_argument` (`crates/providers/mainframe-env-cics/
  src/handlers/file_control.rs`) require `LENGTH`/`KEYLENGTH` as
  `mainframe-env.cics.decimal@1` for every file-control operation, but left
  `STARTBR`/`READNEXT`/`READPREV`/`ENDBR` on the legacy-argument route
  (`execute_legacy`, `crates/kernel/mainframe-env-interpreter/src/machine/
  typed_cics.rs`), which never resolved a `KEYLENGTH(LENGTH OF x)` clause to
  that schema. `STARTBR` therefore failed closed with a generic `ERROR`
  condition instead of ever registering a browse, `READNEXT` failed the same
  way, and the source program's `ENDBR` -- which correctly never suppresses
  `INVREQ` -- then raised it against a browse that had never existed.
  `execute_legacy` now lowers `LENGTH`/`KEYLENGTH` for file-control commands
  the same way typed `READ`/`REWRITE` already do: a `LENGTH OF x` clause
  resolves to `x`'s byte length and a halfword binary reference decodes as a
  whole-number decimal, and a bare numeric `LENGTH`/`KEYLENGTH` literal (such
  as `KEYLENGTH(16)`) now resolves the same way, with no data-name lookup.
  `toreleon/mainframe-env#184`.
- Fixed CardDemo's account-update `SYNCPOINT ROLLBACK` failing with a bare
  `Unauthorized` (surfaced as `host call failed: Unauthorized`) even though
  the compensating dataset rewrite it protects was itself authorized.
  `db2_resources`, `ims_resources`, and `mq_resources`
  (`crates/providers/mainframe-env-{db2,ims,mq}/src/service.rs`) manufactured
  a synthetic "CURRENT" unit-of-work resource and asked the enterprise
  authorizer to approve it for every `Commit`/`Rollback`, even when the run
  unit never opened a Db2, IMS, or MQ unit of work -- a regression from
  `a51f274`'s enterprise-resource authorization, which 0.1.1
  (`44f3081`) predates. `CicsService`'s `syncpoint_db2`/`syncpoint_ims`/
  `syncpoint_mq` call all three unconditionally on every `SYNCPOINT`, so a
  CardDemo transaction that never touches Db2/IMS/MQ was denied trying to
  roll back work it never did. Each `*_resources` function now returns no
  resources (nothing to authorize) when the run has no pending unit of work
  for that provider. Fixing the authorization also exposed a second,
  independent bug in `mainframe-env-ims`/`mainframe-env-mq`'s
  `execute_at`: an untouched Commit/Rollback still persisted a durable
  replay row, which `validate_state`'s `f87eaaa` invariant (state must be
  empty absent an installed definition) then rejected as
  `InfrastructureFailure` in an environment where IMS/MQ have no installed
  definitions. Both now skip replay persistence for a Commit/Rollback only
  when the provider has no installed definitions *and* the run has nothing
  pending -- an installed-but-untouched Commit/Rollback still persists its
  replay row, so a redelivered idempotency key still replays the recorded
  no-op instead of acting on whatever real unit of work the run has since
  opened. `toreleon/mainframe-env#183`.
- Fixed `CicsResume` rejecting the ordinary pseudo-conversational hand-off
  between two different online transactions as a 503
  `infrastructure_failure`. `crates/apps/mainframe-env-server/src/product.rs`
  compared `resume_terminal`'s admitted transaction against the terminal's
  stale pre-resume snapshot instead of re-resolving the online program for
  whichever transaction `resume_terminal` actually admitted -- a regression
  from `00f25a9`'s online transfer-loop rewrite, which dropped the
  `mainframe-env-v0.1.1` (`44f3081`) behavior of always deriving the resumed
  program from `resume_terminal`'s own result. This broke every CardDemo
  online transaction transfer via `EXEC CICS RETURN TRANSID(...)`, including
  `COMEN01C`'s hand-off to `COACTVWC` for the account view.
  `toreleon/mainframe-env#181`.
- Admitted pinned AWS CardDemo (`59cc6c2f`)'s bare 3270-logical
  `EXEC CICS SEND FROM(...) LENGTH(...) NOHANDLE ERASE END-EXEC` -- issued in
  five ABEND-ROUTINE paragraphs, `app/cbl/COACTUPC.cbl:4211`,
  `COACTVWC.cbl:924`, `COCRDSLC.cbl:865`, `COCRDUPC.cbl:1539`, and
  `app/app-transaction-type-db2/cbl/COTRTUPC.cbl:1684` -- through a second
  generated, compiler-only compatibility descriptor bound to the
  pre-existing raw `SendText` route the legacy runtime has executed since
  0.1.1 (`44f3081`). Only the compile gate added in `f8d44ec`/`f1fe39e`
  rejected it: registry row `ibm-cics-ts-6x-2026-08-31:api-commands:0187`
  stays `Unready` and `advertised: false`, per the Syntax section of
  `SSJL4D_6.x/reference-applications/commands-api/dfhp4_send3270logical.html`.
  `toreleon/mainframe-env#177`. The reviewed runtime-operations table is left
  unchanged, because editing it moves source-map, extraction, and review
  receipt digests that cannot be re-verified while `toreleon/mainframe-env#173`
  blocks the source review. Folding row 0187 into that table and deleting this
  descriptor is tracked in `toreleon/mainframe-env#180`.
- Gave every CardDemo conformance-harness check (`toreleon/mainframe-env#175`)
  that compiles an `app/app-transaction-type-db2/cbl` program the same Db2 DCL library
  (`app/app-transaction-type-db2/dcl`, as library `db2-dcl`) and
  `cobol.sql-precompile=true` option that `carddemo_db2_bundles` already gave
  its own callers. Previously only `carddemo_db2_bundles` did this;
  `explicit_carddemo_bundles` -- used by `verify_carddemo_data_layouts_from_env`,
  `verify_carddemo_control_flow_from_env`, `verify_carddemo_core_semantics_from_env`,
  `verify_carddemo_file_call_semantics_from_env`, `verify_carddemo_host_operands_from_env`,
  `verify_carddemo_cics_abi_from_env`, `verify_carddemo_cics_runtime_from_env`,
  `verify_carddemo_vsam_from_env`, `verify_carddemo_batch_programs_from_env`,
  `verify_carddemo_base_batch_from_env`, `verify_carddemo_ims_from_env`,
  `verify_carddemo_mq_authorization_from_env`, and `carddemo_base_online_definition`
  -- built Db2-program bundles without it, so `EXEC SQL INCLUDE DCLTRTYP
  END-EXEC` was never expanded and the DCLGEN group was absent. Since typed
  receiver resolution landed (`229077a`, `00f25a9`), `cargo xtask
  carddemo-operator-install --check` and `cargo xtask carddemo-cics --check`
  failed closed with `carddemo.cics.hir_failed: app/app-transaction-type-db2
  /cbl/COTRTLIC.cbl missing HIR` because `COMPUTE DCL-TR-DESCRIPTION-LEN`
  could not resolve its typed receiver. `explicit_carddemo_bundles` now
  builds every Db2-program bundle with the DCL library and precompile option
  itself, and `carddemo_db2_bundles` is a plain filter over it with no
  duplicated construction; non-Db2 program bundles are unchanged.
  COTRTLIC now compiles; both checks still fail closed, now on
  `app/app-transaction-type-db2/cbl/COTRTUPC.cbl`, on a pre-existing, unrelated
  gap (`InvalidResolvedStatement(ExecCics, 1459, "CICS application command
  SEND is catalog-known but its handler is unready")`) that already
  reproduces on this base commit via `cargo xtask carddemo-db2 --check`,
  which already built COTRTUPC's bundle correctly through
  `carddemo_db2_bundles`; tracked in `toreleon/mainframe-env#177`.
- Accepted `DATASET(...)` as `FILE`'s compatibility spelling on every CICS
  file-control command whose registry row declares a `FILE` option and does
  not itself declare `DATASET`: READ, READNEXT, READPREV, REWRITE, WRITE,
  DELETE, STARTBR, RESETBR, ENDBR, and UNLOCK. Pinned AWS CardDemo `59cc6c2f`
  writes `DATASET(...)` at `app/cbl/COBIL00C.cbl:443` (STARTBR),
  `app/cbl/COCRDLIC.cbl:1129` (STARTBR, plus three more masked sites in the
  same program), `app/cbl/COUSR01C.cbl:240` (WRITE), and
  `app/cbl/COUSR03C.cbl:306` (DELETE); `COTRN00C`, `COTRN02C`, and `COUSR00C`
  were masked by an earlier comment-line failure. Since `f1fe39e` ("enforce
  generated compiler routing") these failed to compile with `CICS <cmd> has
  unknown or unreviewed top-level option DATASET`, because the only existing
  alias -- the shipped typed file plan -- covered just READ and REWRITE. The
  alias is now derived from the registry descriptor (`family: "file-control"`
  plus a declared `FILE` option and no declared `DATASET`) instead of a
  hand-listed command name, following the same compiler-side compatibility
  precedent as `49ae7c9` ("preserve reviewed compatibility routes"); it never
  widens any catalog row, the generator, or the conformance JSON. `FILE(...)`
  and `DATASET(...)` together on one command, and a repeated `DATASET`, are
  still rejected. No cached pinned IBM topic confirms `DATASET` as a
  documented synonym for `FILE`; this is a bounded compatibility alias with
  the same standing as the pre-existing READ/REWRITE one, not a confirmed IBM
  rule. The legacy-compatibility execution route already carried `DATASET`
  through unchanged --
  `crates/providers/mainframe-env-cics/src/handlers/file_control.rs:125-126`
  resolves `DATASET` before falling back to `FILE` for every one of these
  commands -- so no runtime change was needed.
- Stopped a standard fixed-format comment line (`*` in column 7, IBM
  Enterprise COBOL 6.5 Language Reference `rlfmtcom.html`) from reaching
  statement operand/argument text when it sits inside a multi-line
  statement. Pinned AWS CardDemo `59cc6c2f` writes such a comment inside
  an `EXEC CICS ... END-EXEC` option list at seven sites across four
  programs: `app/cbl/CORPT00C.cbl:575,590`, `app/cbl/COTRN00C.cbl:546,597`,
  `app/cbl/COTRN02C.cbl:533`, and `app/cbl/COUSR00C.cbl:541,592`. Statement,
  option, and branch text is sliced from the source by byte span
  (`token_range` in `crates/kernel/mainframe-env-compiler/src/hir/statement_grammar.rs:960`,
  14 call sites); the span slicing dates from `5c09dfa` ("Repair COBOL
  statement grammar and token boundaries"), and comment lines between the
  first and last token of a span were never excluded, so the comment's
  text became part of the resolved text. `f1fe39e` ("enforce generated
  compiler routing") then made the strict CICS top-level clause check
  reject that leaked text with `CICS top-level clause is malformed`,
  failing all four programs (`toreleon/mainframe-env#176`), and masked the
  `DATASET(...)` compatibility gap this file's previous entry fixes for
  `COTRN00C`, `COTRN02C`, and `COUSR00C`. Comment-line bytes (a fixed-format
  column-7 comment normalizes to a floating `*>` comment in
  `normalize_source`, `crates/kernel/mainframe-env-compiler/src/syntax.rs`)
  are now blanked once, before the procedure grammar lexes the source
  (`crates/kernel/mainframe-env-compiler/src/hir/source_text.rs`), so
  every `token_range` call site is fixed at the shared layer with no
  per-call-site change; quoted literals containing `*` or `*>`, and
  comment-free statements, are unaffected. `*` is never stripped inside
  CICS clause resolution itself.
- Restored CICS application-command options whose pinned syntax diagram draws
  the parenthesized operand as an independently optional nested group: bare
  `CURSOR` on `SEND MAP`/`SEND CONTROL`, bare `DATESEP`/`TIMESEP` on
  `FORMATTIME`, and bare `ERRTERM` on `ROUTE` and `FORMFIELD`/`QUERYPARM` on
  `WEB STARTBROWSE` once again compile, matching each option's IBM default
  when its operand is omitted (`dfhp4_sendmap.html`, `dfhp4_formattime.html`,
  `dfhp4_route.html`, `dfhp4_webstartbrowseformfield.html`,
  `dfhp4_webstartbrowsequeryparm.html`, all under
  `SSJL4D_6.x/reference-applications/commands-api/`). `tools/generate_cics_descriptors.py`
  now derives a new `CicsApplicationOptionValueShape::OptionalValue` shape
  directly from that nested-group structure in the pinned syntax projection
  (`conformance/0.9/generated/cics-application-command-contracts.json`)
  instead of any hand-listed option name, so the fix generalizes to every
  option the pinned diagrams mark this way. This unblocks
  `app/cbl/COSGN00C.cbl` and 16 other CardDemo programs that regressed to
  `MECOB0102: "... requires a parenthesized operand"` at `f8d44ec`
  ("freeze 263-command contract and registry") and `f1fe39e` ("enforce
  generated compiler routing"). `app/app-authorization-ims-db2-mq/cbl/COPAUS2C.cbl`'s
  repeated top-level `NOHANDLE` on one `ASKTIME` command (also regressed by
  the same two commits) now compiles too (`toreleon/mainframe-env#171`): an
  exact bare repeat of an option whose registry shape is
  `CicsApplicationOptionValueShape::Flag` is accepted as idempotent, and the
  resolved CICS HIR is identical to the single-occurrence form, because a
  repeated bare flag adds no operand and there is nothing to reconcile.
  Every other repeat — a non-`Flag`-shape option, or any occurrence that
  carries a parenthesized operand — is still rejected exactly as before.
  This is a bounded-ambiguity acceptance the owner can veto in review, not a
  confirmed IBM rule: the pinned sources cached for this contract
  (`dfhp4_apiformat.html`, `dfhp4_asktime.html`) describe NOHANDLE's effect
  but do not state whether repeating it is legal. It would be reversed by a
  pinned CICS translator-message topic that calls a duplicated option an
  error.
- Accepted, evaluated, and executed level-88 condition-names on an alphanumeric
  group item (including a group declared with a mixed-usage subordinate, such as
  a BINARY subgroup), whose entries may precede the group's subordinate items,
  per IBM Enterprise COBOL 6.5 "Format 2" (`SS6SG3_6.5/lr/ref/rlddeva2.html`),
  "Group comparisons" (`SS6SG3_6.5/lr/ref/rlpdsgrp.html`), "Alphanumeric
  comparisons" (`SS6SG3_6.5/lr/ref/rlpdsalp.html`), figurative constants
  (`SS6SG3_6.5/lr/ref/rllancon.html`), "SET for condition-names"
  (`SS6SG3_6.5/lr/ref/rlpssetd.html`), and hexadecimal-notation alphanumeric
  literals (`SS6SG3_6.5/lr/ref/rllitahx.html`); national and UTF-8 groups
  remain outside the executable subset. This unblocks
  `app/cbl/COACTUPC.cbl`'s `WS-EDIT-US-PHONE-NUM-FLGS`,
  `app/cbl/CSUTLDTC.cbl`'s `FEEDBACK-TOKEN-VALUE`, and the corresponding
  group in `app/cpy/CSUTLDWY.cpy` used by `app/cbl/COTRTUPC.cbl`, which
  previously failed `MECOB0101: InvalidDeclaration` at commit `5de3ce4`.
- Moved CICS READ/REWRITE `LENGTH`/`KEYLENGTH` clause lowering, dataset-name lock-conflict
  and delete-lock retention checks, and the CardDemo job-wait/spool-failure helpers into
  sibling modules, and tightened the reviewed module-review production-line ceilings for
  `mainframe-env-dataset/src/service.rs` and `mainframe-env-conformance/src/carddemo.rs`.
- Allowed dataset deletion by the transaction that owns an active allocation lock while
  preserving lock rejection for unrelated transactions.
- Kept a job's exclusive dataset-name lock through its own IDCAMS `DELETE` until step end, so
  the step-end release finds it, while `DISP` terminal deletes still drop the lock with the
  dataset and other transactions still receive `LOCKED` on the reserved name.
- Repaired CardDemo conformance harnesses to run submitted jobs on JES background workers,
  observe terminal state through authenticated z/OSMF routes, and stop workers at gate shutdown.
- Admitted one durable JES work record for each internal-reader child after its worker-run parent
  returns, using the child's own validated-plan capabilities and crash-safe duplicate checks.
- Made CardDemo gates wait for internal-reader child jobs to appear and reach terminal state before
  checking their completion and output.
- Attempted admission for every internal-reader child even after an earlier sibling fails, released
  the parent's own work for a bounded retry on a transient admission failure instead of dead-lettering
  it immediately, and validated an already-admitted child's record against its frozen identity on
  reclaim instead of a fresh, possibly drifted capability recomputation.
- Made legacy EXEC CICS compatibility routes reject source-valid options that
  their pre-typed runtime handlers do not implement, preventing silent operand
  drops while retaining the documented `DATASET` file-name compatibility alias.
- Moved the authentication wall-clock fixture wholly behind the server test
  boundary and tightened the reviewed product-module production-line ceiling.
- Anchored authentication-session expiry and rotation to the shared durable
  clock so wall-clock regressions cannot revoke valid sessions or bypass the
  cross-node per-user quota fence.
- Applied the idempotent PostgreSQL executable-artifact migration from both
  store entry points so fresh shared stores can persist schema-v2 metadata.
- Corrected typed ADD/COMPUTE receiver-local `SIZE ERROR` commits, resolved
  `ADD CORRESPONDING` by relative qualifiers with bilateral uniqueness and
  subordinate-item exclusions, and made selected-table compatibility preserve
  the requested occurrence while invalid or unrepresentable forms fail before
  publication.
- Moved decimal/CICS plan decoding, slot binding, and condition-topology checks
  into dialect-aware HIR/MIR/artifact verification, while retaining defensive
  machine admission.
- Persisted versioned executable manifests with installed artifacts and required
  manifest-aware admission before installed batch, online, nested-call, reload,
  or continuation execution.
- Routed the CICS file/UOW pilot through the durable execution coordinator and
  bounded arithmetic-expression grammar descent before typed HIR construction.
- Persisted bounded typed host audit decisions with versioned canonical resource
  digests; made effect-result, lifecycle, outbox, and audit commits atomic; and
  made ordinary RACF authorization auditing fail closed and survive recovery.
- Derived batch host grants from the validated JCL plan and installed program
  registry, and required typed table/PSB/database/queue SAF authorization
  inside Db2, IMS, and MQ providers before any mutation.
- Routed installed online CICS programs through the durable, resume-aware
  execution journal. Per-session exchange identity now survives restart,
  unresolved effects return the explicit `unknown_outcome` gateway code, and
  file, transient-queue, program-link, and syncpoint replay is provider-ledger
  fenced before a recovered machine can continue. Pseudo-conversation
  checkpoints now close their old execution with an explicit durable handoff,
  and restart cleanup preserves only that handed-off continuation while
  retaining known terminal failure categories.
- Added bounded, transactional retention archives and saturation forecasts for
  lifecycle events, delivered outbox rows, resolved effects, and Db2/IMS/MQ
  replay receipts while protecting checkpoints and unresolved recovery state.
- Resolved PostgreSQL, TLS, bootstrap, and package secrets through one bounded
  reference provider; added named CLI overrides, secure first-administrator
  bootstrap with secret-free restart, and separate writable/auth/artifact/worker
  readiness checks backed by rolled-back provider-state DML proof, retention
  headroom, and per-worker queue-progress freshness.
- Added the PostgreSQL writable-readiness rollback contract to the blocking
  parity stage so every environment-gated PostgreSQL correctness test is run.
- Kept full development certification runnable while preserving stable
  release-artifact checks behind an explicit release-candidate promotion.
- Required the archive CLI to verify its locked offline runtime before granting
  reproducibility credit.
- Kept Jenkins test temporaries below the excluded Cargo target tree so
  parallel fixtures cannot be mistaken for candidate repository contents.
- Moved synchronous z/OSMF backend calls to a bounded four-worker lane and
  propagated finite HTTP deadlines plus live cancellation into invocations.
- Classified mutating-effect journal failures after dispatch as unknown outcomes
  and added fenced, bounded stale-intent recovery across memory, SQLite, and
  PostgreSQL without redispatching the original mutation.
- Enforced PostgreSQL row/object quotas with transactional reservations, moved
  the PostgreSQL product profile to a shared immutable artifact store, and made
  local artifact publication no-replace and directory-durable.
- Moved JES execution out of HTTP submission into a bounded two-worker pool
  backed by a durable monotonic logical clock, generation-scoped FIFO claims,
  preserved JES priority, owner-bound execution contexts, periodic heartbeats,
  fenced lease recovery, and graceful worker shutdown.
- Filtered dataset catalog listings through a discrete SAF decision per name
  and derived pagination hints only from resources visible to the principal.
- Fenced every work-lease transition by a monotonic epoch and observed clock,
  clamped leases to deadlines, and prevented expired queued work from running.
- Made the offline Cargo archive reproducible twice in one digest-pinned GNU
  tar environment and made local and GitHub release assets immutable by digest.
- Bounded and zeroized transient authentication secrets, randomized and
  unified credential policy, and replaced durable raw bearer tokens with
  hashed, rotating, expiring sessions with a durable cross-server user quota
  and non-reusable principal-authentication-epoch revocation.
- Made schema discovery cover every versioned conformance directory and made
  CICS oracle imports validate the 0.9 schema before a closed typed origin is
  parsed or credited.
- Sealed the compiler's executable type-state chain and separated semantic
  artifact identity from the exact payload SHA-256 used by runtime references;
  the artifact contract is now `mainframe-env.artifact@2` and old compiler
  outputs must be rebuilt before execution.
- Replaced diagnostic provider replay identities and lifecycle outbox payloads
  with versioned canonical encodings, including fail-closed legacy
  reconciliation and credential-redacted RACF command digests.
- Unified execution-journal, effect, checkpoint, and artifact invariants across
  memory, SQLite, and PostgreSQL stores, with hostile-record rollback contracts.
- Replaced release-wide Cargo inventory with official-schema-validated
  CycloneDX 1.6 SBOMs for each exact target production closure, including the
  dependency graph; replaced unauthenticated local provenance with a signed
  DSSE in-toto Statement, SLSA Provenance v1 fields, a reviewed Jenkins builder
  identity, unique invocation identity, and tamper-failing verification.
- Expanded the Rust 1.95.0 gate to the complete workspace, all targets, and all
  features; locked Jenkins and external CI inputs immutably; and embedded exact
  supply-chain input identities in offline Cargo bundles.
- Added the complete Apache-2.0 project license and ICU attribution, generated
  deterministic full notices from each target production dependency closure,
  and made dependency-license policy a blocking CI and release gate.
- Made CI discover every shipped Python and shell tooling test, and expanded
  isolated PostgreSQL parity to cover durable migration and CardDemo restart
  suites.
- Made the macOS release build retain its required `LC_UUID` and required the
  exact target CLI and server binaries to pass launch, help, version, and
  readiness probes before release receipts can be written.
- Corrected RACF flat/nested syntax value handling and added generated-path
  regressions.
- Corrected EXEC CICS condition-policy precedence so `RESP` continues to update
  its response area when combined with `NOHANDLE`, while `RESP2` still requires
  `RESP`, on both typed and legacy interpreter paths.
- Bound the existing time handler to `ASKTIME ABSTIME`, whose packed-decimal
  output it implements, and kept bare `ASKTIME` unready until EIBDATE/EIBTIME
  updates exist. Preserved only exact `INQUIRE PROGRAM` legacy SPI compatibility
  through a generated compiler descriptor without exposing other SPI commands.
- Corrected Jenkins checkout/temp storage, tool selection, parameter handling,
  shell portability, and release-target selection.
- Fixed CREASTMT `STEP040` (`PGM=CBSTM03A`) failing `cargo xtask
  carddemo-operator-submit --check` with `ResourceExhausted` before the program
  ran (#182). `hydrate_dds` (`crates/apps/mainframe-env-batch/src/service.rs`)
  embedded each dataset-backed DD's hydrated records twice -- flattened into
  `DdPlan::inline_data` and exactly in `ProgramInput::dd_records` (added by
  `c6b487d`, after `mainframe-env-v0.1.1`'s single flattened copy) -- and
  `execute_program_controller`'s JSON-encoded `BoundedPayload`
  (`mainframe-env.program.input@1`, 1 MiB cap) inflated STEP040's 151,700 raw
  SHR-input bytes to 1,051,400 encoded bytes. `hydrate_dds` now flattens a
  dataset-backed DD's records into `inline_data` only for `SYSIN` and
  `SYSLIB*` -- the only DD names whose flattened bytes are still read after
  hydration (COBOL terminal input and compile-on-run source/copybooks in
  `mainframe-env-server/src/cobol.rs`, and the IDCAMS builtin's control
  statements in `program.rs`); every other dataset-backed DD now carries its
  records exactly once, in `dd_records`.

### Known issues

- Typed terminal plans forward `CURSOR` and `FREEKB`, but the terminal provider
  does not yet model cursor placement or keyboard-lock state (#210).
- These `0.8.3` development changes are not part of the published 0.8.2 tag. The
  [pre-0.9 deep review](docs/reviews/PRE-0.9.0-DEEP-REVIEW.md) records the
  release-truth, durability, security, CI, and documentation blockers that must
  be resolved before 0.9.0 implementation and publication.

## [0.8.2] - 2026-09-06

### Fixed

- Retained installed COBOL program state within a run unit, observed live
  cancellation and deadlines, preserved unknown outcomes, and made installed
  child replay durable and stable without redispatching completed calls.
- Required verified HIR before executable lowering.
- Rejected incomplete, non-progressing, or changing DCOLLECT catalog traversals
  instead of publishing partial output.
- Applied one provider-state Move validation contract across memory, SQLite,
  and PostgreSQL.

### Evidence and distribution

- Separated observation perturbations from behavioral mutants in dataset
  certification schema `@2` and added an opt-in memory-store scaling benchmark.
- Published a locked offline Cargo vendor bundle and SHA-256 checksum after an
  offline workspace build. No 0.8.2 native binaries or binary receipts were
  published.

### Compatibility

- This patch changes runtime and contract behavior. Custom `WorkStore`
  implementations must add `get_work`; legacy MIR must be recompiled; and
  protocol-1 or counter-era in-flight installed calls require draining or
  explicit reconciliation before protocol 2.
- DCOLLECT can now fail where 0.8.1 returned partial output. Provider Move maps
  missing memory sources to `Conflict` and oversized SQL payloads to
  `PayloadTooLarge`. Dataset certification consumers must accept schema `@2`.

## [0.8.1] - 2026-09-05

### Fixed

- Preserved durable abend state and stable per-step effect identities across
  warm restart, preventing inverted `COND` handling and duplicate `DISP=MOD`
  appends in multi-step jobs.
- Preserved exact utility record boundaries, including empty records and data
  containing `0x0A`, without delimiter-based reconstruction.
- Serialized absent-dataset probe/create under a durable name reservation and
  retained the original abend when terminal DD cleanup also fails.
- Corrected omitted abnormal `DISP` defaults, CCSID-aware fixed-record padding,
  negative zoned edit signs, content-addressed spool rollback, JES queue use,
  de-hardcoding scan scope, and local artifact hygiene.

### Compatibility

- The program-input wire contract gains only an optional typed record map.
- 0.8.1 accepts 0.8.0 checkpoint identities. Queued and terminal durable jobs
  migrate through defaults; an in-flight legacy step without a replay base
  fails closed and can be resubmitted instead of risking a duplicate effect.

### Known limitations

- The licensed z/OS 3.2/JES2 differential remains exactly 0/16 pending under
  the approved `pass-with-licensed-differential-pending` policy. Hercules,
  MVS 3.8J, modeled behavior, local output, and generated or historical
  evidence receive zero licensed equivalence credit.
- The authentic licensed campaign remains a hard gate for 0.17 release
  certification and 1.0, while the standalone receipt adapter remains
  fail-closed.

## [0.8.0] - 2026-09-04

### Added

- Added deterministic JES2 scheduling, DD allocation and DISP processing,
  artifact-backed spool, output routing, started tasks, internal readers,
  bounded NJE/MAS topology, operator controls, and durable recovery.
- Added admission-pinned typed program registrations and real bounded semantics
  for all nine required utility families without program-name dispatch or
  generic-success fallback.
- Certified the pinned CardDemo base-batch corpus across 3 journeys, 12
  initialization jobs, and 9 operational jobs.

### Known limitations

- The licensed z/OS 3.2/JES2 differential remains exactly 0/16 pending under
  the approved `pass-with-licensed-differential-pending` policy. Hercules,
  MVS 3.8J, modeled behavior, local output, and generated or historical
  evidence receive zero licensed equivalence credit.
- The authentic licensed campaign is a hard gate for 0.17 release
  certification and 1.0, while the standalone receipt adapter remains
  fail-closed.

## [0.7.0] - 2026-09-02

### Added

- Added the lossless bounded JCL frontend, catalog-driven validation, procedure
  and symbol expansion, immutable typed planner, and JES2 JECL annotations.
- Added exact recognition and validation for all 237 pinned JCL/JES2 rows with
  deterministic malformed, recovery, scale, compatibility, and CardDemo plan
  matrices.

### Known limitations

- JCL row-wide execution, condition, recovery, and licensed differential gates
  remain pending; 0.7 is a converter/planner release, not the 0.8 JES runtime.
- CD-006 and CD-013 receipts are stale. Fixed-format comment text leaks into a
  multi-receiver `MOVE` during CardDemo online execution. IDCAMS rejects
  CardDemo's `DELETE ... CLUSTER` and `DATA/INDEX(NAME(...))` component forms,
  so the reproduced online and batch journeys fail. These defects are not
  hidden by weakened gates; 0.7.1 is the planned patch.

## [0.6.0] - 2026-09-02

### Added

- Added typed dataset, VSAM organization, catalog, GDG, allocation, locking,
  RLS/TVS, migration, backup/restore, and recovery semantics.
- Added generated execution for the 31-command AMS surface and local
  independent reference-model assurance across all 36 official rows.

### Known limitations

- The licensed z/OS 3.2 dataset/VSAM/AMS differential remains exactly 0/36 and
  is deferred to release certification.

## [0.5.0] - 2026-09-02

### Added

- Added the complete pinned RACF command and RACROUTE/SAF surface through one
  typed, durable, authorization-aware authority.
- Added audit redaction, migration/recovery, replay, restart, credential/MFA,
  certificate/keyring, and independent reference-model assurance.

### Known limitations

- The licensed z/OS 3.2 RACF/SAF differential remains exactly 0/48 and is
  deferred to release certification.

## [0.4.0] - 2026-09-02

### Added

- Added deterministic execution for the pinned COBOL statement, intrinsic,
  data, file, JSON/XML, condition, and recovery surfaces.
- Added checkpoint schema 10 and the bounded 16-case GnuCOBOL reference
  campaign as local assurance with zero licensed credit.

### Known limitations

- The licensed Enterprise COBOL 6.5 differential remains exactly 0/153 and is
  deferred to release certification.

## [0.3.0] - 2026-09-02

### Added

- Added the shared typed Conformance IR, deterministic shard/cache identities,
  replayable verdicts, and derived ledgers.
- Added complete recognition and validation coverage for the pinned 173-row
  COBOL structure and type-system inventory.

### Changed

- Generalized stable `0.x.y` release preparation and bound post-0.2 receipts to
  the clean live source tree while retaining immutable 0.2 evidence.

## [0.2.0] - 2026-09-02

### Added

- Added reviewed official coverage catalogs, generated registries, application
  packages, subsystem ABI libraries, and six independent evidence gates.

### Changed

- Authorized external promotion of the immutable accepted 0.2 candidate and
  its two retained target receipts; this branch does not create the tag or
  publish artifacts.

## [0.1.1] - 2026-08-31

### Added

- Accepted the bounded mainframe-env 0.1 greenfield product contract.
- Added the deterministic ME.V0 scope, oracle, profile, package, and evidence entry pack.
- Added Rust 2024 workspace, release-version authorities, and architecture/profile checks.
- Certified the generic COBOL, CICS, Db2, IMS, MQ, dataset, JES, RACF, restart,
  backup/restore, security, and overload capabilities with the complete
  CardDemo corpus. CardDemo remains conformance data and tooling, not a shipped
  application feature.
- Added owned, hash-pinned `COCRDSEC` demo source for the upstream `CDV1`
  orphan with an explicit no-card-data correction contract.

### Changed

- Promoted the product and all workspace crates to 0.1.1.
- Corrected corpus-tooling `CARDEMO` spellings to `CARDDEMO`. The typo in the
  pinned upstream FTP JCL remains only as an explicit compatibility alias.

## [0.1.0-alpha.0] - Unreleased

Initial development identity. This version is not published and makes no production-readiness claim.
