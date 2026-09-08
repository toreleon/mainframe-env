# Local Jenkins CI

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
brew install jenkins-lts
rustup toolchain install 1.98.0 --component clippy,rustfmt
rustup toolchain install 1.95.0
brew install postgresql@18
brew services stop jenkins-lts 2>/dev/null || true
tools/jenkins/bootstrap-macos.sh
tools/jenkins/run-local.sh
```

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
the pipeline fails closed unless the pinned toolchain, Rust 1.95.0, `rustfmt`,
and `clippy` are already installed. Install them before starting Jenkins with
the commands above; those host prerequisites are not Jenkins-managed caches.

## One-time Jenkins setup

Open <http://127.0.0.1:8080> and install Pipeline, Git, and Credentials Binding
(all are in Jenkins' suggested plugin set). Install GitHub Branch Source when
the job must discover pull-request merge refs automatically. No Timestamper or
Pipeline Utility Steps plugin is required. Create a Pipeline or Multibranch
Pipeline job named `mainframe-env`. Select **Pipeline script from SCM**, choose
Git, use this repository URL, set the branch to
`*/main` for a single Pipeline job, and set the script path to `Jenkinsfile`.
Keep the checkout non-shallow because conformance verifies tags and historical
phase commits. The checked-out repository must contain the Jenkins migration;
a job cannot load an uncommitted `Jenkinsfile` from Git.

The Jenkinsfile polls the configured SCM every five minutes and schedules a
weekly full run. `auto` uses `CHANGE_ID`,
`CHANGE_TARGET`, and the previous Jenkins commit when available. Set
`BASE_SHA` to a full commit SHA for a manually checked-out change when Jenkins
does not provide comparison metadata.

Install PostgreSQL 18 tools on the node (`brew install postgresql@18` on macOS).
When the changed-path plan selects the store obligation, Jenkins automatically
creates a disposable cluster under `$WORKSPACE/.postgres`, runs the ignored
backend parity contracts, and stops the cluster. Missing or non-18 PostgreSQL
tools fail the selected stage instead of turning it into a skip.

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
