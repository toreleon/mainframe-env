# CI supply-chain inputs

`Jenkinsfile` is the only current CI workflow. Its executable inputs are
governed by two reviewed locks:

- `tools/ci-inputs.lock.json` fixes the workspace and MSRV toolchains by
  release and compiler/Cargo commit, fixes every separately installed CI tool
  by an exact version, and inventories tracked action, container, and package
  installation inputs;
- `tools/jenkins/controller-plugins.lock.json` fixes the Jenkins LTS WAR and
  the complete 63-plugin closure by exact version and SHA-256.

`tools/supply_chain.py check` enumerates files with `git ls-files`. A tracked
GitHub action must use a 40-hex commit, a tracked container must use an
`@sha256:` digest, and tracked CI cannot install ambient packages. The current
inventory contains no GitHub actions or package-install commands and exactly
one Linux/amd64 archive image pinned by digest. The Jenkins controller, plugins,
Rust toolchains, Cargo dependencies, archive image, and host tools are locked
inputs.

## Deliberately unsupported local files

The following names have existed as locally excluded experiments on some
workstations, but they are not in Git and are not CI or release authority:

- `.github/workflows/offline-dev-bundle.yml`;
- `tools/build_offline_dev_bundle.sh`;
- `tools/package_offline_bundle.sh`;
- `tools/offline_bundle_assets/`; and
- `tools/offline_dev_assets/`.

The lock records those names and the validator requires them to remain
untracked. Their container tags, package commands, and behavior are therefore
not claimed as pinned. Promoting any of them requires deliberately adding the
files, replacing every mutable input, updating the tracked-input inventory,
and reviewing the resulting workflow. Historical GitHub Actions evidence under
`conformance/` remains historical data, not an active workflow.

## Install and verify Jenkins

Create the capped volume, then download the exact reviewed artifacts into it:

```bash
tools/jenkins/bootstrap-macos.sh
"$(tools/jenkins/select-python.sh)" -B tools/supply_chain.py install-jenkins \
  --home /Volumes/MainframeEnvJenkins/jenkins-home
```

The installer downloads only the versioned Jenkins WAR and versioned plugin
URLs, checks every SHA-256 before installation, verifies artifact manifest
identities, and verifies that every required plugin dependency is inside the
locked closure. It refuses an unreviewed active plugin and creates a pin marker
for every accepted plugin. `run-local.sh` verifies
the same bytes again and launches the locked WAR directly; it does not run a
package-manager shim. Jenkins also reruns repository, runtime, controller, and
plugin validation as a blocking `supply-chain` gate.

The host must already provide the exact versions in `tools/ci-inputs.lock.json`.
The lock currently requires Rust/Cargo 1.98.0, Rust/Cargo 1.95.0 for MSRV,
Python 3.12.13, Git 2.50.1, Docker 29.4.3, Java 21.0.12.1, cargo-deny 0.20.2,
PostgreSQL 18.6, and GitHub CLI 2.92.0. Install cargo-deny only from its immutable Cargo package
coordinate:

```bash
cargo +1.98.0 install cargo-deny --version 0.20.2 --locked
```

Package-manager commands are workstation provisioning, not CI steps. A
different installed version fails before it can receive assurance or release
credit.

## Offline release input identity

`tools/package_offline_cargo_bundle.sh` first verifies the locked offline tool
scope. Each resulting archive contains:

- the two reviewed lock files; and
- `SUPPLY-CHAIN/BUILD-INPUTS.json`, which binds the tag commit, Cargo lock,
  Rust toolchain file, CI locks, complete vendored-tree bytes and modes, and the
  versions plus executable SHA-256 values of Cargo, rustc, Git, Python, and the
  container runtime.

The record also binds the exact Linux/amd64 container image digest and the GNU
tar 1.34/gzip 1.12 pair inside it. Packaging copies the source tree into two
clean directories with deliberately different mtimes, normalizes member order,
time, ownership, mode, extended metadata, and gzip headers, and requires both
archives to have the same SHA-256 before publishing either. Existing local or
GitHub assets are retained only when their bytes match; different bytes fail
closed and are never clobbered.

## Reviewed updates

Every input update is a normal reviewed repository change:

1. State the security, compatibility, or maintenance reason and select exact
   versions from the upstream release records.
2. Download the candidate Jenkins WAR and complete required plugin closure into
   a disposable directory. Verify upstream signatures where available, compute
   SHA-256 locally, inspect plugin manifests, and update both version and hash.
3. Update `reviewed_on`; never update only a version or only a digest, and never
   use `latest`, a moving branch, a container tag, or an unbounded package
   install.
4. Run the installer and verifier against a disposable Jenkins home, then run:

   ```bash
   "$(tools/jenkins/select-python.sh)" -B tools/supply_chain.py check --runtime ci
   cargo +1.95.0 check --workspace --all-targets --all-features --locked
   cargo test --workspace --all-features --locked --no-fail-fast
   cargo deny check
   cargo xtask license-notices --check
   ```

5. Record the change in `CHANGELOG.md`, obtain review, and update a live
   controller only after the lock change is accepted. Rollback is the prior
   lock and its already reviewed artifacts.
