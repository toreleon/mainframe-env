# ADR-0028: Explicit BTS SET loans in the checked interpreter

Status: **Proposed; bounded root proofs recorded; parent acceptance pending**
Owner: **Interpreter and selected execution maintainers**
Scope: **explicit BTS GET CONTAINER SET in one root machine**
Applies from: **mainframe-env 0.9.0 development**

## Context and decision

The existing channel loan label tracks a channel/container pair. BTS source
authority instead maintains a SET area until the task issues another GET
CONTAINER SET or ends. Two independently seeded compiled root regressions
showed that a successful different-container SET and a handled missing-container
SET left the old LINKAGE readable. INTO and NODATA correctly preserved it.

Keep allocation and checked pointer access in the existing interpreter bases and
freed-allocation set. Private pending metadata distinguishes a channel pair from
an explicitly selected BTS SET and captures its prior owned base and bounded
capacity. ACTIVITY, PROCESS, ACQPROCESS and ACQACTIVITY follow actual provider
selection; omitted-selector and current-channel ambiguity are not inferred.

Expire only the captured prior BTS loan at the matching observed CICS response,
after pending effect identity validation and before SET allocation or response
outputs. A handled missing-container response reaches this boundary. A host
error, mismatched result, deadline/envelope rejection or effect quota failure
does not prove a CICS issue and does not expire it. INTO/NODATA and channel
loans remain separate. Capture prevents a stale observation from freeing a
newer base; duplicate resumes retain the existing pending-result rejection.

Normal SET must provide a bounded payload and matching fullword FLENGTH before
publishing a fresh loan. Record its existing allocated base with a private
freed label in the current snapshot representation. Preflight charges retained
bytes, both projected bases and freed entries; checked address and live storage
limits still apply. No new arena, allocator, coordinator, provider namespace,
public ABI or snapshot field is introduced.

## Compatibility and limits

The snapshot profile and codec stay unchanged. Newly labeled active and expired
root loans round-trip through the existing checkpoint. Historical unmarked BTS
areas cannot be safely identified and are not arbitrarily freed. Older readers
can parse the representation but do not enforce the new BTS label semantics.
Downgrade therefore requires draining affected tasks or retaining a compatible
interpreter and verified backup; parsing compatibility is not lifetime proof.
No historical checkpoint is rewritten on read.

This bounded repair does not provide task-wide virtual address sharing,
cross-frame LINK/RETURN/XCTL ownership, task-end release, implicit BTS selection
or cold process/backend recovery. Root checkpoint observations do not establish
Recovered coverage. Those contracts and parent CIC-904 acceptance remain pending.

## Authority and verification

Catalog row `ibm-cics-ts-6x-2026-08-31:api-commands:0086`, GET CONTAINER, EIBFN
`3414`; baseline `ibm-cics-ts-6x-application-api-sources-a-2026-09-10`.
`SSJL4D_6.x/reference-applications/commands-bts/dfhp4_getcontainerbts.html`,
SHA-256 `e486de8019489b85dbcf0b0f343849d0cc088a331c03054db7329aa1da58ad16`: explicit
selectors 19–37/77–80, fullword FLENGTH 41–68, SET lifetime 81–88 and missing
container 94–97. The distinct channel lifetime is
`SSJL4D_6.x/reference-applications/commands-api/dfhp4_getcontainerchannel.html`,
SHA-256 `e699b5c003cedd015c05ec116bd05e5cdd46e8fce2f0bd070559555984386674`, 152–178.
Both sources were searched and hash-verified with the repository reader before
semantic edits. Publication reference earns zero execution or licensed credit.

Require six compiled selected root regressions with independent payloads, checked
ordinary LINKAGE dereference and FLENGTH 65,536, focused observation/control/
bounds/restore negatives, unchanged channel controls and mandatory policy gates
on the integration candidate. DATA-EXCEPTION 2/0 is the existing checked memory
outcome, not a claimed IBM abend or RESP mapping. Official row, recovered and
licensed acceptance remain pending; differential credit is zero.
