# User and maintainer guides

Practical paths through the sandbox and framework. Start with an executable
workspace or one local program, then choose an embedding or server composition.

[Documentation portal](../README.md) · [Project status](../../README.md#project-status)

| Guide | Use it to |
|---|---|
| [Mainframe Sandbox](MAINFRAME-SANDBOX.md) | Set up an executable agent workspace and use CardDemo through CLI, MCP, or a browser |
| [Capabilities and limitations](CAPABILITIES.md) | Evaluate supported surfaces and evidence limits |
| [Embed mainframe-env in Rust](EMBEDDING.md) | Compose compiler, runtime, providers and stores in Rust |
| [Getting started](GETTING-STARTED.md) | Run and inspect a local COBOL program |
| [Framework glossary](GLOSSARY.md) | Understand framework and mainframe terms |
| [Public source distribution](DISTRIBUTION.md) | Review documentation and distribution claims for the current source |

## Maintain documentation

Use task-focused guides for onboarding and versioned architecture/contracts for
normative behavior. Give a new normative document its status, owner, scope,
and applicable subsystem/contract scope near the title, then register it in
`docs/documentation-registry.json`. Preserve historical candidate identities.

Write diagrams as fenced `mermaid` blocks. Use flowcharts for dependencies and
pipelines, state diagrams for lifecycles, and sequence diagrams for protocol
ordering. Label arrows when their meaning could be confused with dependencies.
Keep nearby prose or tables sufficient to understand the behavior without a
renderer. Quote labels containing punctuation and avoid external images,
renderer-specific themes, scripts, and diagram links. GitHub renders Mermaid
blocks; other Markdown viewers may show the source until Mermaid is enabled.

Edit registry-owned navigation in the registry, preserving the portal and
subsystem generation markers. Regenerate with `cargo xtask docs`, then run
`cargo xtask docs --check`. Add a unique changelog fragment and run the
[documentation policy checks](../../CONTRIBUTING.md#verification-scope).
Preview diagrams and execute changed runnable examples against the candidate.
