# ADR-0040: Private primary level witnesses and search boundary

Status: **Proposed**
Owner: **IMS provider maintainers**
Scope: **IMS-1403.primary-level-position-closure, reviewed private design**
Applies from: **mainframe-env 0.14.0 development**

The existing primary traversal must distinguish its actual next-search boundary
from the occurrences that satisfied each SSA level and from the returned/held
current. A failed data-qualified search can examine B13 while only B11 satisfies
the intermediate SSA. Computing feedback or U/V parents from current ancestry
after failure cannot represent that history. This phase adds no alternate
traversal, PCB registry, feedback scan, shared request, coordinator or authority.

An optional private `primary_search` V1 member behind PcbPosition records the
selected metadata digest, observed revision/allocation high-water, exactly three
contiguous satisfied occurrence slots, examined/missing order address and captured
feedback path. Each occurrence carries its original ID, parent, segment, raw key
and observed version. The boundary carries raw root-to-node keys, node/subtree
edge, examined/deleted provenance and capture generation; it is never fabricated
current or hold. Before-first/end and missing-key variants remain distinct.
Deletion provenance comes only from the actual successful DLET removed set and
pre-deletion order path, never from interpreting an unexplained missing reference. Historical validation
does not require archived records to survive; live validation requires exact
occurrences and non-regressed high-water. Equal keys/IDs do not certify restore.

The metadata digest is SHA-256 over domain
`mainframe-env.ims-primary-search@1\0` then deterministic JSON of existing selected
PSB name, PCB number, PCB metadata and database metadata. It is neither source
authority nor an image incarnation. Strict readers reject unknown/duplicate
fields, versions, bad widths/topology, noncontiguous slots and incompatible
secondary/restart/after-end state. At most three keys occur per path. The bound
is checked `45 * max_segment_bytes + 8192` plus existing row/state/host capacity;
no lower runtime key cap, schema namespace or ratchet is introduced. Recovery's
separate encoded-key limit remains unchanged. Absent fields reserialize with
byte-identical historical fields and confer no U/V witness certification.

Finite new admission is existing typed/public-provider and signed-selected
DbBatch GN/GHN, primary HIDAM, exactly three fixed physical levels with unique
named sequence fields, multiple physical types allowed. Exactly three unqualified
SSAs describe a real metadata path, terminal has no active code, and parent codes
are root U, intermediate U, both U, single root V or single intermediate V.
V expands to U at that level and above for this call only. Lower U releases when
its ancestor moves. Inactive null slots retain canonically distinct raw bytes.
Source-selected live witnesses are required; legacy ancestry cannot supply them.
The actual ordinary prefix/full GU/GHU and qualified GN/GHN produce witnesses
in the same sole loop, with unique EQ ancestor fences and unconsumed type probes.
Ordinary SSA GN/GNP consumes the certified boundary. Finite legacy reads use the
same matcher/loop under trusted Batch Invocation; their requests have no explicit
DbBatch context. Other legacy producers clear certification and retain their
legacy selection owner. Two-level F/L keep
their existing private FirstInParent/Last owner and allocate no three-level trace.

Exclude logical participation anywhere in the full catalog or retained links,
secondary, other organizations/contexts, unkeyed/nonunique/variable keys, GO/E/Q,
key-only, omitted/qualified/terminal U/V, mixed/duplicate codes, F/L/path mixtures
and unpositioned/unresolved witness histories. These remain unfinished required
classes, not accepted applicability exclusions. Qualified U/V precedence and
F/L ancestor reset facts are source-backed but not admitted by this phase.

Real failed GN A=A1, B>=B11 AND BDATA=14, C=C113 captures satisfied A1/B11 and
examined B13. Independent ordinary next GN must return E11f; independent U/U or
intermediate V must return C111a. Exact-parent A1/B11/C113 failure ends after
C112, so ordinary GN returns D111e. These are reviewed source-derived finite
compositions, not a claim that the topic's inconsistent earlier root-GE/GU prose
is an executable IBM example. Only a fresh attempted get cancels same-PCB hold;
successful Get Hold alone creates a real versioned selected hold. Replay retains
its original canonical output and never re-executes or recreates a witness/hold.

Checkpoint captures the certified satisfied feedback prefix, never examined B13.
Actual CHKP clears positions/holds. Actual XRST performs a new traced GU against
the saved key without hold; restored prefix B11 has B11 parentage, and restored
child has child parentage. Missing GU captures its actual satisfied prefix and
requested keyed gap in that same loop, without predecessor/prefix recovery scans
in this finite class. Old IDs and holds are never revived by saved keys.

HISAM integration at `86bf917e5a20d581a3b5d56b89de86db94ccadf4` transfers
database/store.rs, service/generic.rs and service/execution.rs to the same primary
closure lane. Successful finite ISRT captures the actual inserted path and order
boundary, without hold; its parent comes from satisfied levels or a real traced
parent GU. Failed parent qualification publishes that GU's actual GE prefix.
REPL refreshes actual observed versions while retaining key/path/boundary and the
existing hold/update owner. DLET truncates only removed certified levels, records
the immutable subtree gap, reconciles the deleting run's other PCBs and uses the
existing reset owner for other runs. Historical feedback keys may survive deleted
occurrences and need not equal today's live levels. Equal-key reinsertion cancels
affected old gap certification, including legacy insertions; only another real
positioning call can certify the new occurrence. Duplicate/quota/non-search
conditions retain their existing status/state owners; broader II-before-duplicate
and sentinel/FIRST/HERE/raw/variable classes remain separate unfinished work.
Load, backout, reschedule and whole-position resets cancel certification; no
restored image or equal ID/key can resurrect it. Actual basic/symbolic CHKP clears
all PCB positions and holds, and exact CHKP replay preserves newer position.
The first fresh-feedback transport exposed an existing shared validity boundary:
old host-api `ImsPcbFeedbackResultV1::validate` requires blank status for every
Valid key. Manager review authorizes a bounded repair in that owner and its
canonical tests: Valid on blank success, or GE with empty result segments and
zero transferred bytes. GE with segments, other failed statuses and all existing
name/level/byte/bound/checkpoint/system violations remain rejected. No DTO/tag or
canonical encoding changes. Shape validation does not establish provenance: the
fresh private witness, capacity and atomic proposal remain IMS-owned. Historical
Unsupported failed-witness decoding/bytes remain unchanged. This decision remains
Proposed and supplies no HUMAN/official or pending raw CALL/TM approval. Old host
readers reject GE/Valid: stop/drain/reconcile and compatible readers or coherent
pre-feature restore are necessary; no field stripping, eager rewrite or rolling
mixed-writer/downgrade equivalence is admitted.
No retained feedback snapshot substitutes for a fresh call-local witness. AC/AM
condition receipts, SAF, canonical raw identity, CAS/capacity, lost ack, undo,
participant and retention owners remain authoritative. No raw CALL/TM approval
under pending ADR0031/0033 is supplied by this private design.

Sources: IMS 15.6 `ims-ssa-position-commands`, baseline
`ibm-ims-15.6-ssa-position-commands-2026-09-11`, U topic
`SSEPH2_15.6.0/com.ibm.ims156.doc.apr/ims_ucmdcode.htm`, SHA-256
`dd714045f66d410138eaacc9c0cae5d2da9d2c18d35913f31ba041db60950613`,
5269 bytes, plain-text 3–16/34–53; V `ims_vcmdcode.htm`, SHA-256
`30556749e146ef15dacf630a10c5538186d06765657a236351a7babef2de8b26`,
3557 bytes, 3–9/25–35. `ims-ssa-last-position` failed topic
`SSEPH2_15.6.0/com.ibm.ims156.doc.apg/ims_currentpostafterfail.htm`, SHA-256
`992855fdf264472dfc4e40d28dd81cfce9429feb1b9712e0f4a23bc0b8898726`,
16090 bytes, 3–23/54–78/113–152 supplies the separate histories. Existing
programming GN/GHN 90–206, GU/GHU 90–167, PCB mask 159–183 and recovery XRST
164–200/basic CHKP 51–57/symbolic CHKP 69–85 supply the retained/get/hold
composition. Actual retained-path-first SHA/byte verification, repository parser
and registered search/read receipts remain external and grant zero execution
or accepted IR/HUMAN/official/licensed/participant credit. Catalog denominator
stays 25; rows `:0005/:0006/:0015` REPL, `:0023` symbolic CHKP and `:0025` XRST
apply; `:0016` RETRIEVE remains a separate obligation. Registered sequential ISRT
scope `ims-sequential-isrt-position`, baseline
`ibm-ims-15.6-sequential-isrt-position-2026-09-11`, ISRT SHA
`3095a5cc7d24ec775a2dbc36463d171e87d896ba21783db6d8efd13f1d871d82`
(26140 bytes, lines 129–166/202–213), position SHA
`6788ba399b8bba78a7af93fc0788c07535c57bcd77739bbb6e61aebfa8cba521`
(4327 bytes, lines 3–16/29–31), and II SHA
`5ac05c96282a80fa09efa34f48335ccfdd0947e713a4dce6d922976d5f3fb3fc`
(3397 bytes) constrain the finite insert composition without closing broader II.

Older strict readers reject the additive field. Before downgrade stop admission,
drain holds/UOWs, reconcile unknown effects and retain a compatible reader or
restore a coherent pre-feature backup of database/session/undo/recovery/replay/
package/journal/audit references. Never strip live fields, mix writer versions,
or restore independently selected equal-byte rows as witness proof. The existing
load/backout/reset fences, coherent backup boundary and reserved retention prefix
are necessary; this field is not a mixed-image certificate or cleanup family.
Separate-process SQLite reopen proves only selected local durable behavior.
Scoped PostgreSQL parity and coherent durability are required for this completion
leaf; wider backup certification and IMS/v0.14 obligations remain separate. Seal
only after every covered producer/consumer/proof and required gate passes.
