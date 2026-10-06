# CardDemo READACCT run bundle

Set `CARDDEMO_CORPUS_DIR` to the clean pinned upstream checkout described in the
[operator runbook](CARDDEMO-OPERATOR.md), then run:

```bash
cargo xtask carddemo-readacct --check
```

The gate executes the real READACCT source and JCL through the framework,
validates the live run bundle against the retained schema, verifies its logical
replay digest, and validates projections against five digest-pinned modernize-ai
schemas offline. It prints return code, output count, replay digest, and the
current wall-clock interval. Both command modes perform the same live validation;
neither writes a historical receipt into the repository.

Capture the command output outside Git when reviewing a candidate. Source and
expected fixture identities remain checked inputs. The self-recorded local
projection has development authority and supplies no independent conformance
credit; see [ADR-0026](../decisions/0026-run-bundle.md).
