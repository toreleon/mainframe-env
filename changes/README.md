# Changelog fragments

Parallel feature pull requests must add one unique TOML file under
`changes/unreleased/` instead of editing `CHANGELOG.md` directly. Use a stable,
lowercase work-package or issue identifier as the file name:

```toml
schema_version = "mainframe-env.changelog-fragment@1"
category = "added"
summary = "Added a bounded capability without changing existing routes."
```

Allowed categories are `added`, `changed`, `deprecated`, `removed`, `fixed`,
and `security`. The summary is one trimmed line of at most 512 bytes and does
not include the Markdown bullet prefix.

A fragment records an implemented change awaiting changelog integration. Its
presence does not mean the feature is unimplemented, or that its subsystem has
passed every acceptance gate. Keep the current implementation scope, remaining
work, and unavailable checks in the owning progress record. The
[unreleased status](../docs/delivery/UNRELEASED-STATUS.md) links those records and
defines their status vocabulary. Do not add status fields to the fragment schema.

Run `cargo xtask changelog --check` in feature worktrees. During release or
batch integration, run `cargo xtask changelog` once: it inserts all fragments
into the Unreleased section, removes the consumed fragment files, and
regenerates the documentation manifest. Review and commit that integration diff
as one metadata change.

The documentation manifest has a repository merge driver because independent
branches can still add or update different documentation paths. Install it once
per clone with:

```bash
python3 -B tools/setup_git_merge_drivers.py
```

The driver merges document rows by path and recomputes aggregate counts. It
stops on competing edits to the same document, which remain a real review
conflict.
