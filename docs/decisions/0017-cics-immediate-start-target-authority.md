# ADR-0017: Resolve immediate START targets from installed durable definitions

Status: **Proposed for cics.application-api development; command rows pending**
Owner: **CICS and server maintainers**
Scope: **START ATTACH and START BREXIT transaction admission**
Applies from: **mainframe-env current subsystem contracts**

## Context

The pinned CICS TS 6.x application baseline `ibm-cics-ts-6x-2026-08-31`
defines `START ATTACH` at catalog `api-commands:0206`, topic
`dfhp4_startattach.html`,
`sha256:ebda4b40984414871bfa2fb294104dc5aefa81599a99351c94cdb4974f3d4b3a`,
and `START BREXIT` at catalog `api-commands:0207`, topic
`dfhp4_startbrexit.html`,
`sha256:6456007b2abdcf4042a6aac59ae5ab653652b406e9e65b6b1fe886eafe9543b1`.
Both exact retained raw HTML files were hash verified and parsed with the
repository PlainText parser before this change. The source defines TRANSIDERR
28 for an undefined target and response2 11 for a remote target.

The server already owns immutable `online-transaction` and `online-program`
installation rows in the shared durable store. A CICS start handler must
resolve the target before scheduling; otherwise a missing target can be
reported as a successful start and silently discarded by the worker later.

## Decision

1. The CICS service reads the existing installation rows as a read-only
   authority. It accepts a local 1–4-character transaction only when its
   version-one transaction row names a canonical installed program, its
   program row names a canonical artifact, and the bound artifact authority
   verifies the executable payload and manifest metadata.
2. An absent transaction returns TRANSIDERR 28/0. A malformed or inconsistent
   retained row returns infrastructure failure. Neither result enqueues work.
3. A source-defined default BREXIT name is stored immutably in a canonical
   `cics-bridge-default-v1` row bound to one installed local transaction.
   Explicit BREXIT names override that default; a missing default returns
   PGMIDERR 27/0. Exit selection verifies the installed executable artifact,
   and CICS open rejects malformed default rows.
4. These lookups grant no START ATTACH or START BREXIT route by themselves. Those
   routes still require live-address and bridge-exit authorities, respectively.

## Consequences

The provider reuses the server's installed local transaction identity instead
of maintaining a second catalog. Memory and SQLite reopen produce the same
answer. Remote transaction definitions are not present in this local catalog;
future remote admission must represent them explicitly and return the source
TRANSIDERR 28/11 condition. No licensed execution credit follows from this
read-only boundary.

## Verification

Focused memory and SQLite reopen tests cover undefined, valid installed, and
corrupt durable rows, plus explicit/default exit selection, immutable replay,
and malformed default rows. Module, API documentation, typed-semantic,
documentation, formatting, dependency, and diff gates apply.
