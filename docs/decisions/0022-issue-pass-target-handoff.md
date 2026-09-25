# ADR-0022: Retain and claim an ISSUE PASS target handoff

Status: **Proposed for v0.9 development**
Owner: **CICS terminal and Communications Server adapter maintainers**
Scope: **CIC-905 ISSUE PASS and target CICS EXTRACT LOGONMSG**
Applies from: **mainframe-env 0.9.0 development**

## Context

ISSUE PASS disconnects a terminal after the source task ends and transfers it,
with at most 255 bytes of logon user data, to the named z/OS Communications
Server application. If the target is another CICS system, EXTRACT LOGONMSG can
read that data when LGNMSG is enabled. The pinned CICS TS 6.x sources are row
0126 `dfhp4_issuepass.html` in baseline
`ibm-cics-ts-6x-application-api-sources-b-2026-09-10`, SHA-256
`2b4fd4acfa3485a857180ff9124110b36185402308e8f0e4a72ed296d3baaeae`, and row
0071 `dfhp4_extractlogonmsg.html` in baseline
`ibm-cics-ts-6x-application-api-sources-a-2026-09-10`, SHA-256
`6ea9d8e4eb3a93414a39a329fddd342390048b3779d576a8c00380fc59098c94`.
Their retained raw HTML matched the committed hashes and was parsed offline.

## Decision

1. The existing physical `IssueDeviceRecord` remains the PASS authority. The
   source task stages target, data, logmode, and NOQUIESCE; known task commit
   atomically disconnects its session and makes the transfer claimable.
2. A trusted target adapter claims the transfer by terminal, target run, and
   stable event ID. The target run's APPLID must equal LUNAME and SAF must
   authorize the facility. The device row retains the claimant and event in
   one CAS update. An exact retry returns the same bounded transfer; a second
   claimant or event fails.
3. If the target is CICS and LGNMSG is enabled, the adapter publishes the
   transferred bytes to the existing task-local EXTRACT metadata. A retry
   repairs a claim committed before metadata publication. Once EXTRACT
   consumes the bytes, a retry does not restore them.
4. The claim API represents target-side acceptance by a trusted adapter. It
   does not infer real Communications Server delivery from local persistence.
   The external carrier, failure notification, selected compiled source path,
   and remaining source conditions must pass before row 0126 is registered.

## Consequences

- The optional claim fields decode older canonical device rows unchanged.
- A source task cannot PASS twice under a different target without a new
  authorized device state transition. The target cannot claim before task
  completion or after another target has claimed.
- Memory, SQLite reopen, and PostgreSQL concurrent-provider regressions cover
  the bounded claim. This is backend evidence, not licensed VTAM evidence.
