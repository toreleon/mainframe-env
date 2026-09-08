# Compiler and IR Architecture

Status: **Accepted by repository owner**

## Goals

- Preserve exact source bytes, encoding, copybook/precompiler expansion, and
  provenance.
- Support analysis of incomplete programs without allowing incomplete state to
  become executable.
- Model language-specific meaning without forcing unrelated languages into one
  universal operation set.
- Make storage, aliasing, decimal behavior, control flow, conditions, and host
  effects explicit.
- Provide one reference path that can be interpreted and analyzed without a
  second semantic authority.
- Keep every compiler stage deterministic and bounded.

## Source model

`SourceBundle` owns the complete semantic input closure:

```text
primary source bytes
declared encoding and source format
copybooks/includes and exact bytes
precompiler inputs and versions
compiler options and dialect
logical paths and content identities
expansion/provenance graph
```

Source identity is computed from exact bytes and declared semantic metadata.
Physical absolute paths, timestamps, and directory enumeration order are not
semantic identity.

Host ABI copybooks are ordinary explicit source-library inputs. Their CICS,
Db2, or MQ provider owns exact bytes, version, license, and provenance; the
compiler neither embeds those assets nor chooses a subsystem generation.

Decoding does not discard byte provenance. Mainframe sources may originate in
EBCDIC or fixed-column formats, so diagnostics retain a mapping between logical
characters, original byte ranges, and expansion origins.

## Syntax strategy

The authoritative compiler frontend uses:

1. language-aware source preprocessing;
2. a handwritten lexer where column, encoding, or contextual state matters;
3. an event-based or recursive-descent parser;
4. a lossless Rowan-style concrete syntax tree;
5. typed AST wrappers over the lossless tree; and
6. an independent semantic model.

Tree-sitter may provide editor tooling, but its tree is not compiler authority.
Parser-combinator or generated-parser libraries may be used for bounded local
grammars and protocols, not imposed as a universal frontend framework.

## Stage guarantees

| Stage | Guarantee |
|---|---|
| `LosslessSyntax` | Represents every input token/trivia byte and recovery node |
| `ParsedProgram` | Typed syntax views exist; errors may remain in analyze mode |
| `SemanticProgram` | Names, layouts, types, scopes, calls, and effects resolved where possible |
| `VerifiedHir` | HIR invariants hold; unknown/recovery nodes explicitly non-executable |
| `LegalizedMir` | Every executable operation has a registered schema and backend route |
| `PublishedArtifact` | Immutable bytes and compatibility metadata committed atomically |

Parsed and semantic construction is private to the compiler implementation.
The executable proof chain consumes verified HIR into lowered MIR and then
legalized MIR; publication encodes that legal module internally. Semantic
artifact identity uses `semantic-sha256:`, while the exact payload digest alone
uses the runtime `sha256:` artifact-reference namespace.

This distinction is artifact contract `mainframe-env.artifact@2`. Version 1
compiler outputs used a semantic digest in the `sha256:` namespace and are not
silently reinterpreted; they must be rebuilt so every executable reference can
be verified against the exact payload bytes.

Analyze mode may return partial syntax, AST, semantic, or HIR results with
diagnostics and completeness metadata. It may never construct a publishable or
executable artifact.

## IR object model

The IR framework provides:

- typed, stable-width IDs backed by deterministic vector arenas;
- modules, regions, blocks, operations, operands, results, attributes, and
  source locations;
- explicit storage regions, extents, aliases, overlays, and references;
- structured and CFG control-flow forms;
- ordered effect declarations;
- namespaced operation and type identities with dialect-major versions;
- provenance chains from generated operations to original source bytes; and
- resource limits checked before allocation and during verification.

The in-memory IR does not depend on a persistence codec. Codecs are adapters
that translate through a validated envelope.

## Dialects

The common framework does not define a language enum. Dialects own operation
and type families, for example:

```text
cobol.hir
mainframe.core
mainframe.memory
mainframe.decimal
mainframe.control
cics.terminal
cics.file
cics.program
db2.sql
jcl.workflow
```

A language may retain a valid independent HIR while still sharing lifecycle,
diagnostics, effects, host capabilities, artifact publication, and execution
contracts.

## Operation catalog

Operation metadata is declared once in a readable, versioned catalog. A catalog
entry owns:

```text
operation identity and dialect version
operand/result constraints
attribute schema
effects and ordering
required host interfaces and capabilities
verification and legalization rules
supported reference-interpreter route
documentation and support state
```

Deterministic generation may produce Rust descriptors, schema documents,
registry entries, and support documentation. Generated
bindings are never the only readable statement of the contract.

Execution handlers and lowering implementations are written explicitly and
registered against catalog identities. The build fails when a supported
executable operation lacks verification, legalization, or a selected backend
route.

## COBOL organization

COBOL is large enough to require separate syntax and semantic/compiler
responsibilities. ADR 0005 keeps them as private modules in one 0.1 compiler
crate because they have no independent consumer or publication boundary.

```text
mainframe-env-compiler::syntax
  source formats
  preprocessor and COPY expansion
  lexer
  lossless CST
  typed AST

mainframe-env-compiler::{semantic,hir,lower}
  semantic analysis
  layout and storage
  HIR construction and verification
  lowering by operation family
  compiler plugin facade
```

Lowering is organized by domain:

```text
lowering/
  context.rs
  control.rs
  data.rs
  arithmetic.rs
  string.rs
  file.rs
  program.rs
  cics.rs
```

A `LoweringContext` owns builders, layouts, symbols, CFG state, limits,
diagnostics, and provenance. Domain modules do not reconstruct or bypass those
invariants.

## Reference backend

The deterministic MIR interpreter is the only 0.1 execution backend and the
semantic oracle. Symbolic, Cranelift, LLVM, and Wasm backends are post-0.1
decisions and introduce no 0.1 implementation obligation.

An optimized backend is promoted only when:

- every emitted operation is supported;
- exact semantic differential passes;
- overflow, encoding, decimal, alias, condition, and effect behavior matches;
- cancellation and resource limits remain observable; and
- fallback is explicit rather than automatic and silent.

If a native backend is added later, its types do not appear in public artifacts
and its API remains confined to an adapter crate.

## Artifact identity

Artifact identity is a SHA-256 digest over a canonical semantic manifest that
includes:

- exact source/dependency content identities;
- compiler and frontend generation;
- normalized compilation options;
- target and backend contract;
- IR/dialect contract versions;
- precompiler and code-generation inputs; and
- required host interface versions.

The hash is not computed from ordinary Protobuf, JSON, Rust debug, or map
serialization output. Artifact payload bytes have their own integrity digest.
