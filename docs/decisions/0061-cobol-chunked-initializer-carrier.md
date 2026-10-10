# 0061. Versioned bounded COBOL initializer byte carrier

Status: proposed. This changes local executable representation, not IBM language meaning or acceptance completion.
Owner: compiler, IR and interpreter maintainers
Scope: byte-preserving large initializer lowering, artifact admission and whole-view initialization
Applies from: mainframe-env local mainframe.core.cobol@2.init byte carrier

The existing local frontend computes a complete initializer vector. A single IR Bytes attribute is bounded to 1 MiB; the compiler and invocation admit storage up to 64 MiB. A 32768 by 64-byte table therefore fails at representation admission before execution, even though its 2 MiB storage is inside the original storage bound.

The new named operation identity carries the same already-computed vector in canonical ordinal chunks, each no larger than the unchanged per-attribute limit. Exact metadata, consecutive ordinal vocabulary, every part length, total byte coverage, one named whole target view and MemoryWrite shape are checked by the dialect-owned IR validator. Parts infer their offsets; arbitrary gaps, overlaps and alternate ordering are not wire inputs. Raw-value meaning remains the frontend's authority.

The interpreter assembles the complete vector before its existing whole-view fit/write. Ordinary initialization, alternate ENTRY, PSAPTR initial context and ALLOCATE INITIALIZED share this decode. Existing retained-entry overrides still precede writes; snapshot bytes and padding/truncation behavior are unchanged. Multiple partial init writes are prohibited because the existing write fits and overwrites a whole view.

Small initializers retain mainframe.core.cobol@1.init and exact existing attributes. Existing artifacts remain accepted. New large artifacts derive both core dialect majors when other operations remain major 1; artifact readers require that exact manifest set. Old catalogs/executors refuse the unknown major-2 operation, and no reader silently downgrades it to init@1. Installing or copying an artifact continues to bind its actual payload identity and complete manifest metadata through the existing generic artifact/store contracts.

No IR model, codec/envelope, artifact, command-output, time or storage limit increases. Parts and their original framing must fit those same limits. No repeat-compression or implicit fill byte substitutes for the offered vector. Local controls compare all 2097152 independent 0x20 bytes, use a nonuniform boundary/order oracle and reject malformed part metadata. The unchanged original scale source and public CLI run remain the future runtime proof; a direct IR fixture supplies no memory-scale application credit.

Offline COBOL 6.5 VALUE, OCCURS and program-initial-context topic bodies/TOC remain unavailable at their committed pins. This carrier does not change their interpretation, and earns no IBM semantic parity, language coverage, runtime scaling result or full Foundation acceptance by itself.
