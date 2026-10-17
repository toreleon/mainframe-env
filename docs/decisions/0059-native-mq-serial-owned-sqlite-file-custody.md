# ADR0059: serial owned SQLite file custody for native MQ succession

Status: proposed infrastructure contract; controls and native producer unrun.
Owner: SQLite store adapter and configured native MQ root maintainers
Scope: serial born-owned SQLite file custody for explicit native MQ scope succession
Applies from: mainframe-env optional Unix owned-file native MQ custody profile

ADR0034's actual compiled-root authority and ADR0057/0058's irreversible old
writer cutoff remain required. Reopening a SQLite URL, finding equal rows,
copying a root claim or asserting matching IDs does not admit another root.

The bounded Unix profile creates its file exclusively from birth with safe
`std::fs::OpenOptions::create_new`. The adapter privately retains that actual
read/write `File`, its device/inode, a canonical absolute UTF8 path and one-link
identity. Existing files, copies and symlinks cannot enroll. Ordinary URL opens
and other backends retain their original contracts and provide no owned-file
custody. No unsafe code, native SQLite handle or new dependency is introduced.

The actual privileged native manager first derives ADR0057 custody from its
retained compiled frame/root and actual storage-admitted Running execution.
Before physical reopen it requires the exact originating concrete adapter Arc
to equal its retained host's PlatformStore Arc. Another adapter on the same
file, a copied store or a different genuinely owned driver is refused before
the actual source pool is closed. Application CALLers cannot mint this custody.

The adapter checks its safe held File and named regular file device/inode,
one-link identity and canonical path, then observes its sole SQLx connection's
`pragma_database_list`: the actual main path must match; another attachment is
refused, apart from SQLite's unnamed temp. These checks run before initial
migration writes and before/after a physical reopen. They are observations in a
trusted serial namespace, **not an atomic security proof against concurrent
hostile path replacement**. The privileged owner must serialize all file,
directory and external-adapter changes for this profile. Unsupported platforms
and observed alias/replacement conditions refuse, rather than acquire authority.

Before pool closure, the owning SQLite transaction repeats ADR0058's complete
bounded cutoff comparison: actual fenced root and version1 marker, epoch, all
old actors, core rows, provider Exact/Absent dependencies, indexes and audits.
It also verifies the source Open document's exact next Uncertain row and exact
cutoff-marker encoding. The same full comparison runs on the reopened driver
before a fresh host can escape. The shared helper keeps all predecessor
admission CAS checks; no graph, audit, actor or byte bound is removed.

The old actual pool closes irreversibly and `Pool::close` must finish, draining
every real checked-out connection. Closure is bounded by that pool's original
acquire timeout. The reopened pool uses the internally retained typed path,
safe File, original finite row/payload/archive limits, one-connection pool and
original SQLx defaults. Failure after closure retains fencing and unknown work,
returns no successor and supplies no automatic retry. No connection or native
service mutex is borrowed across this closure by the selected Clock callback.

The actual manager then installs a genuinely new selected service/control and
private cutoff on that reopened adapter. The actual compiled independent root
coordinator owns its fresh version1 admission, with ADR0058's exact shared-scope
version CAS and old-row durable pins. Old transport gates, native frames and ABI
aliases remain revoked; their old pool is physically closed. No MQ UOW is
committed, backed out, acknowledged Terminal, imported or reassigned.

Independent structural controls cover full graph equality before/after real
reopen, old-pool writes, duplicate reopen, six forged-capture refusals, existing
copied/link database enrollment, identical-byte path replacement and symlink,
plain URL refusal, actual other driver and unexpected attachment. Their quotas
remain the original1024 rows/1MiB. Structural actors are not compiled proof.

A separate new native producer uses the existing provider setup and actual
compiled MQROOT, MQNEXT and byte-identical original MQCOLD body. Its revised
owned-file input proves an actual storage-admitted outer root and separately
compiled child, actual Completed/DONE reply, exact two CONNECT receipts, source
registry epoch+1, original generation/fence, exact CALL ownership and encoded
reply, preserved old unknown core/work and fenced root, no MqQueue SAF, old
native/ABI revocation and post-Unknown provider epoch equality. Genuine manager
negative controls refuse same filename, copied rows and another born-owned
driver, retaining both whole native fixture state and all old authority guards.

The historical source05 `physical_reopen_genuine_cold_connect_fences_old_native_points_and_aliases`
source and failure remain retained. Its handwritten outer Caller and plain URL
reopen supplied no new owned-file custody. A separate changed-input amendment
to the original named test replaces those inputs with the actual born-owned
manager and compiled `MQNEXT` caller. It retains every original callback trigger,
completion/DONE reply, exact2 receipt, unknown, old-alias and no-second-publication
assertion, and directly observes the actual unchanged private-provider reply.
It additionally compares source-frozen raw CALL/result bytes, the real admitted
root/child and every preserved old dependency/core row; missing, altered and
multiple observed replies refuse. Old host/router/frame aliases remain the
revoked owners while durable postchecks use the genuinely reopened store.
Unsupported physical custody remains a visible refusal, with no Memory fallback
or skipped required backend. These changed inputs do not relabel the historical
failure, and the original named acceptance requires actual execution with all
unchanged assertions. Compile/native/controls are unrun here; no Foundation,
application or IBM acceptance is claimed. Lost acknowledgement, crash custody,
pending PUT/full removed GET, repeated succession and pinned MQ9.4MQI/COBOL6.5
semantics remain separate work.

Source anchors: retained SQLx0.9 pool `close`/`is_closed`, pool options
`connect_with`/`get_acquire_timeout`, SQLite `filename`/`create_if_missing`;
`mainframe-env-store/src/sqlite.rs` actual initialization/default limits;
ADR0057/0058 and actual configured custody/NativeRootHooks; existing native
Clock/source/refusals; selected `ObjectRow<Control>` codec and CALL replay v2.
External source pins bind the exact bodies used for this draft and qualification.
