# Local Jenkins CI

For the containerized development environment with automatic local deployment
and a separate disk boundary below 50 GB, use the
[Docker development guide](../../docker/README.md). This runbook describes the
existing host-based full-assurance controller and its 10 GiB boundary.

`Jenkinsfile` is the current CI definition. It keeps the changed-path plan and
command receipts used by the repository, runs the complete tier weekly or on
demand, and handles the release offline-bundle path explicitly.
Historical GitHub Actions receipts under `conformance/0.2/` remain historical
evidence; they do not describe current CI.

## Capped storage

Jenkins must run on a dedicated filesystem whose total capacity is no larger
than 10 GiB. On macOS, create and mount the repository's default APFS sparse
bundle, then start Jenkins in the foreground:

```bash
rustup toolchain install 1.98.0 --component clippy,llvm-tools-preview,rustfmt
rustup toolchain install 1.95.0
rustup toolchain install nightly-2026-09-01 --profile minimal --component rust-src
cargo +1.98.0 install cargo-deny --version 0.20.2 --locked
tools/jenkins/bootstrap-macos.sh
"$(tools/jenkins/select-python.sh)" -B tools/supply_chain.py install-jenkins \
  --home /Volumes/MainframeEnvJenkins/jenkins-home
CARGO_HOME=/Volumes/MainframeEnvJenkins/cargo-home \
  cargo +1.98.0 install cargo-fuzz --version 0.13.2 --locked
CARGO_HOME=/Volumes/MainframeEnvJenkins/cargo-home \
  cargo +1.98.0 install cargo-llvm-cov --version 0.9.1 --locked
tools/jenkins/run-local.sh
```

Java, Python, Git, PostgreSQL, and the GitHub CLI are preinstalled host tools;
their exact accepted versions are in `tools/ci-inputs.lock.json`. The pipeline
rejects a version change. See [CI supply-chain inputs](CI-SUPPLY-CHAIN.md) for
the installation boundary and reviewed update procedure.

The default paths are:

```text
$HOME/Library/Application Support/mainframe-env/jenkins-10gb.sparsebundle
/Volumes/MainframeEnvJenkins/jenkins-home
/Volumes/MainframeEnvJenkins/cargo-home
/Volumes/MainframeEnvJenkins/tmp/controller
```

Use `MAINFRAME_ENV_JENKINS_IMAGE` and `MAINFRAME_ENV_JENKINS_VOLUME`, or the
scripts' `--image` and `--volume` options, to choose other paths. A Linux node
may use a loopback filesystem, LVM logical volume, ZFS dataset with a hard
quota, or another dedicated filesystem, provided its reported capacity is at
most 10 GiB.

The sparse bundle or equivalent filesystem supplies the hard limit. The
pipeline refuses to run when that filesystem reports more than 10 GiB or when
`JENKINS_HOME`, workspace, Cargo home, Cargo target, or temporary build files
escape it. Jenkins logs and archived artifacts are under `JENKINS_HOME`.
Disposable PostgreSQL data, sockets, and logs stay below the build workspace.
Targets are deleted after every run, only five build records and one artifact
set are retained, and Cargo download/source caches are pruned once the volume
reaches 75%. Those cleanup rules improve headroom; they are not the hard cap.
Development and test debug information and incremental compilation are disabled
to keep a clean full build within the cap.

The startup helper installs a Jenkins init hook that fixes the controller at one
executor. This serializes all jobs, including jobs created by a Multibranch
Pipeline, so they cannot compete for the capped volume or the disposable
PostgreSQL port.

The controller may read already-installed rustup toolchains from the host, but
sets `RUSTUP_AUTO_INSTALL=0` so a job cannot grow `~/.rustup`. Before any build,
the pipeline fails closed unless the exact locked compiler and Cargo commits
for 1.98.0 and 1.95.0, `rustfmt`, `clippy`, and `llvm-tools-preview` are already
installed. The MSRV gate checks the full workspace with every target and
feature enabled. Full assurance also requires the pinned fuzz nightly plus
`rust-src`, cargo-fuzz 0.13.2, and cargo-llvm-cov 0.9.1. Install them before
starting Jenkins with the commands above; the Cargo helper binaries live in the
capped Cargo home while rustup toolchains remain host prerequisites.
The startup scripts also select an already-installed Python that can load its
standard `hashlib`, `math`, and `ssl` extensions. Set `MAINFRAME_ENV_PYTHON` to
an absolute interpreter path to override the default `~/.local/bin/python3`,
then `/usr/bin/python3`, probe order. The supply-chain gate requires its exact
locked version; the selector never installs packages. Set `MAINFRAME_ENV_JAVA`
to the exact locked Java executable when it is not in a standard local path.

## One-time Jenkins setup

Do not install suggested plugins or update plugins through the web UI. The
repository installer provisions Pipeline, Git, Credentials Binding, GitHub
Branch Source, and their complete dependency closure from the version-and-hash
lock; startup and every build reject missing, disabled, changed, or extra active
plugins. Open <http://127.0.0.1:8080> and create a Pipeline or Multibranch
Pipeline job named `mainframe-env`. Select **Pipeline script from SCM**, choose
Git, use this repository URL, set the branch to
`*/main` for a single Pipeline job, and set the script path to `Jenkinsfile`.
Keep the checkout non-shallow because conformance verifies tags and historical
phase commits. The checked-out repository must contain the Jenkins migration;
a job cannot load an uncommitted `Jenkinsfile` from Git.

For a controller that must remain completely local, use a `file://` URL for the
existing checkout. `run-local.sh` enables Git plugin local checkouts explicitly;
the controller listens only on `127.0.0.1`, and Jenkins still clones into its
capped workspace before running any gate.

The Jenkinsfile polls the configured SCM every five minutes and schedules a
weekly full run. `auto` uses `CHANGE_ID`,
`CHANGE_TARGET`, and the previous Jenkins commit when available. Set
`BASE_SHA` to a full commit SHA for a manually checked-out change when Jenkins
does not provide comparison metadata.

Install the exact PostgreSQL version in `tools/ci-inputs.lock.json` on the node.
When the changed-path plan selects the store obligation, Jenkins automatically
creates a disposable cluster under `$WORKSPACE/.postgres`, runs the ignored
provider-move, canonical-effect, stale-effect recovery, atomic-invariant,
work-lease fencing, migration/durable, and CardDemo restart contracts, and
stops the cluster. The database is dropped and recreated before every contract
so one suite cannot satisfy or contaminate another. Missing or version-drifted
PostgreSQL tools fail the selected stage instead of turning it into a skip.

Every build-bearing plan also records the `python-tooling-tests` and
`api-docs` gates.
`tools/run_tooling_tests.py` discovers tracked `tools/tests` directories at any
repository depth, runs every Python and shell test file, and syntax-checks all
shipped shell tooling. Adding a new versioned conformance tool test therefore
does not require another Jenkinsfile edit. `tools/check_public_api_docs.py`
enables Rust's `missing_docs` lint for every contract crate and rejects any
increase over the reviewed per-crate baseline in `tools/public-api-docs.json`;
an improvement must lower that baseline in the same change, so documentation
debt cannot silently return.
Markdown-only plans remain bounded to the documentation-system gate.

For optional GitHub Release publication, create a Secret Text credential named
`mainframe-env-github-token`. Publication occurs only when both `release` mode
and `PUBLISH_GITHUB_RELEASE` are selected. The tag must already exist, contain
the Jenkins migration and all pipeline helper scripts, and point at its matching
`VERSION`; Jenkins does not create product tags. Tags that predate this migration
remain covered by their immutable historical release evidence and are rejected
before checkout rather than failing later with missing helpers.
Release receipts support `aarch64-apple-darwin` and
`x86_64-unknown-linux-gnu`. An empty `RELEASE_TARGET` uses the host only when it
is one of those targets; other hosts must select the supported cross target and
have its Rust target and linker installed.

## Running the job

Use **Build with Parameters** in Jenkins:

- `auto`: changed-path PR/main assurance;
- `full`: all workspace, documentation, conformance, certification,
  evidence-seal, runtime, mutation, MSRV, and PostgreSQL parity gates;
- `release`: full assurance for a post-migration tag, native release receipt
  reproduction, and a verified offline Cargo vendor archive.

The full tier receipts bounded and periodic cargo-fuzz runs for the COBOL
parser and IR decoders, the registered Loom durable-state model, and an LLVM
line/function coverage summary for the IR, compiler, and store packages.
Coverage JSON and any fuzz crash artifacts are archived with the command
receipts. Kani and TLC are not installed or credited; their current boundaries
and prerequisites are recorded in the verification strategy.

With a Jenkins API token and the Jenkins CLI jar, the equivalent commands are:

```bash
java -jar jenkins-cli.jar -s http://127.0.0.1:8080/ -auth USER:TOKEN \
  build mainframe-env -s -v -p RUN_MODE=auto
java -jar jenkins-cli.jar -s http://127.0.0.1:8080/ -auth USER:TOKEN \
  build mainframe-env -s -v -p RUN_MODE=full
java -jar jenkins-cli.jar -s http://127.0.0.1:8080/ -auth USER:TOKEN \
  build mainframe-env -s -v -p RUN_MODE=release \
  -p RELEASE_TAG=mainframe-env-vX.Y.Z
```

Every selected command writes an exact-candidate receipt below
`target/ci-assurance` while running. Jenkins archives the receipts before it
deletes the workspace target directory.
