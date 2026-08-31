# Execution Prompt — Implement mainframe-env 0.9.0

Target version: **0.9.0**
Completion dependencies: 0.4.0, 0.5.0, 0.6.0

Use this prompt from the repository root. The
[common execution contract](README.md#common-execution-contract) is normative.

---

You are implementing **mainframe-env 0.9.0: complete CICS application API** for
all 263 pinned CICS TS 6.x application commands.

## Read and verify first

Read `docs/prompts/coverage-versions/README.md`,
`docs/delivery/coverage-versions/0.9.0.md`, the generated CICS API catalog,
CICS resource/EIB/condition contracts, and accepted 0.4.0 COBOL host ABI, 0.5.0
SAF, and 0.6.0 data-authority evidence. Verify all three dependency gates before
public integration.

## Implement in this order

1. Freeze **CIC-901** generated grammar, command/option legality, resource keys,
   EIB/RESP/RESP2, conditions, effects, handler registration, and limits.
2. Implement **CIC-902/CIC-903** program/task/interval/storage/recovery,
   terminal/BMS, TSQ/TDQ, file, journal, and spool command families.
3. Implement **CIC-904** channels/containers, BTS, documents, web/HTTP,
   transforms, business transactions, and supported event APIs.
4. Implement **CIC-905** APPC/MRO conversation state and distributed program
   link through transport-neutral contracts.
5. Implement **CIC-906** SAF, concurrency, syncpoint, cancellation, recovery,
   scale, malformed/limit, compatibility, and licensed differential suites.

## Reuse and architecture guardrails

- Build the 263-command application API on one shared CICS command runtime that
  owns generated identities, option legality, resource keys, conditions,
  EIB/RESP mapping, bounds, effect metadata, and exhaustive handler closure.
  Later SPI/FEPI work must extend this runtime rather than fork it.
- Reuse Tower/HTTP and reviewed transport libraries for web, sockets-facing,
  timeout, limit, and tracing adapters. Convert immediately to typed CICS
  requests; transport libraries never define CICS conditions or transaction
  semantics.
- Reuse the accepted dataset, SAF, program, session, checkpoint, UOW, migration,
  package, and evidence authorities. TSQ/TDQ, file, journal, spool, and program
  commands must not hide provider-private stores or retry policies.
- APPC/MRO and distributed-link behavior remains a transport-neutral owned
  protocol state machine. Do not substitute a message broker's delivery or
  acknowledgement semantics for the pinned CICS contract.

## Version-specific invariants

- Generate and exhaustively register all 263 commands; every accepted option
  affects semantics and every missing handler fails explicitly.
- Preserve exact EIB, RESP/RESP2, HANDLE/IGNORE/NOHANDLE and condition behavior
  across normal, failure, cancellation, syncpoint, and restart paths.
- File and security behavior use the 0.6/0.5 authorities; host calls use the 0.4
  ABI. No duplicate resource state or provider-local authorization is allowed.
- APPC/MRO and DPL state is bounded, recoverable where required, and independent
  of application transaction/program names.
- SPI and FEPI completion remain out of scope until 0.10.

## Completion gate

Do not finish until 263/263 application commands pass all applicable coverage
gates; option, EIB/response, condition, terminal, resource, conversation,
syncpoint, cancellation, malformed, bound, authorization and recovery matrices
pass; licensed CICS TS 6.x application-interface differentials pass; and
CardDemo transactions/maps/resources remain exact without production hardcode.

At handoff, provide per-family/per-gate counts, generated catalog and registry
digests, failure/recovery and oracle evidence, and full validation on the exact
candidate. Do not include SPI/FEPI rows in the completion numerator.
