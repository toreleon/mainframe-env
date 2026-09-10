# Docker development environment

This stack provides the Rust development toolchain, local Jenkins CI/CD, and a
persistent PostgreSQL 18 application instance. It supports native Apple Silicon
and Intel containers. The provided macOS bootstrap uses Colima 0.10.3 and Lima
2.2.0, with the existing Docker CLI and Compose plugin.

## Start

Install Colima and Lima if they are missing. Then, from this checkout:

```bash
docker/dev init
docker/dev up
docker/dev cargo build --workspace --all-features --locked
docker/dev test
docker/dev shell
```

For Codex desktop work, keep editing this shared checkout and run development
commands through the wrapper. `AGENTS.md` requires container execution for
builds, tests, linters, generators, and dependency tools:

```bash
docker/dev exec uname -s
docker/dev cargo fmt --all -- --check
docker/dev exec python3 -B tools/run_tooling_tests.py
docker/dev cargo xtask docs --check
```

The first command must report `Linux`. `exec` passes arguments directly to a
temporary container, disables TTY allocation for automation, and preserves the
development cache mounts and host UID/GID mapping. Use `docker/dev shell` for
interactive commands. If the VM or mount is unavailable, repair it with `init`
and `up`; do not switch to host Cargo or Python. Worktrees must be shared with
Colima explicitly so validation runs against the intended checkout.

This is the desktop agent's required workflow, not a host execution sandbox.
Host editing, Git, and Docker/Colima management remain available. New Codex
sessions load repository instructions at startup; this device also has a scoped
rule in its Codex home guidance. See the official
[AGENTS.md documentation](https://learn.chatgpt.com/docs/agent-configuration/agents-md).

The initial image build downloads the pinned Linux toolchains and compiles the
locked Cargo tools. Later builds reuse image layers. Jenkins starts its first
build automatically, then polls the **local Git repository's `main` branch**
every five minutes. Commit or merge changes into local `main` to trigger CI and
local deployment. Uncommitted working-tree edits are available to the development
container, but are not deployment candidates. To poll GitHub directly instead:

```bash
MAINFRAME_ENV_CI_REPOSITORY=https://github.com/toreleon/mainframe-env.git docker/dev up
```

Use the same environment override on subsequent `up` commands to retain GitHub
polling. The pipeline blocks deployment if any workspace or PostgreSQL check
fails, regardless of which source is selected.

Jenkins is at [localhost:18080](http://127.0.0.1:18080/), with username `admin`.
Its generated password is in
`/Volumes/MainframeEnvDocker/secrets/jenkins_password`. The application appears
at [localhost:10443/zosmf/info](http://127.0.0.1:10443/zosmf/info) after the first
successful deployment. Its administrator is `ADMIN`, and its initial password
is in `/Volumes/MainframeEnvDocker/secrets/admin_password`. Passwords are created
once with restrictive permissions and never written into Git or image layers.
Jenkins account changes made through its UI survive restarts.

For the private GitHub repository, `init` reads the existing host Git credential
helper into the private `secrets/github_credentials` file and seeds the Jenkins
credential `mainframe-env-github`. It never prints the token or includes it in
an image. Prefer a repository-scoped read token; replace this credential in
Jenkins to rotate or narrow it. If the helper has no credential, public checkout
still works anonymously; private checkout requires adding that credential in
Jenkins. Source files and Git credentials are mounted only where needed.

Ports bind only to loopback. HTTP is appropriate to this local development
instance; see [the operations runbook](../docs/runbooks/OPERATIONS.md) for TLS
and deployment outside this machine. PostgreSQL has no published host port.

## Capacity and mounts

The hard outer boundary is a **44 GiB APFS sparse bundle**, approximately
47.25 GB before filesystem overhead, below the requested decimal 50 GB limit.
Its filesystem is mounted at `/Volumes/MainframeEnvDocker`. Colima's 6 GiB root
disk, 32 GiB Docker data disk, downloaded VM images, logs, generated secrets,
and caches all live inside it. Sparse allocation grows with actual use.
The bundle defaults to `~/.local/share/mainframe-env/docker.sparsebundle`.

The wrapper checks the actual mounted capacity and reserves 3 GiB before
starting builds. Containers also check their backing filesystem capacity and
headroom. Cleanup helps avoid disk-full failures; the APFS filesystem enforces
the outer limit even if a process writes faster than cleanup. Running out of
space fails a build instead of expanding the VM. The source checkout and
pre-existing host outputs are outside this budget; the existing host `target`
is neither copied into the image nor used by container builds.

| Mount | Purpose | Retention |
|---|---|---|
| Source bind at `/workspace` | Editable working tree | Host-owned; source edits persist |
| `dev-cargo`, `dev-target` | Development downloads and compilation | Reused; disposable |
| `dev-output`, `dev-postgres` | Container `dist` and parity scratch data | Persistent; separate from host outputs |
| `ibm-docs` at `/ibm-docs` | Digest-pinned IBM publication cache for development | Persistent; never pruned by `clean` |
| Source bind at `/source` | Jenkins Git checkout source | Read-only |
| `ci-cargo`, `ci-target` | Jenkins downloads and compilation | Reused; disposable |
| `jenkins-home` | Credentials, jobs, checkouts, logs, JVM temporary files | 5 build records; 2 archived receipts |
| `postgres-data` | PostgreSQL 18 `/var/lib/postgresql` | Persistent; never pruned |
| `releases` | Current and previous application binaries | Last 2 successful binaries |
| `/tmp`, `/run` | Temporary files and bootstrap material | Bounded RAM filesystems |

The development container runs commands with the host user's UID/GID. Jenkins
and the application run as UID 1000. Development and CI use separate caches and
exclusive file locks. Incremental compilation and debug information are disabled
by default; Cargo uses two build jobs. Compiler/download caches are reclaimed at
75% filesystem usage between commands. After image builds, unused BuildKit cache
is pruned with a 2 GB retention target. Container logs rotate at 10 MB × 3 per
service. `max_wal_size` bounds normal PostgreSQL checkpoint WAL growth but is not
a database quota; database data shares the hard disk boundary.

Docker operations target the dedicated daemon by socket. They do not switch the
active Docker context or prune another Docker Desktop installation. Jenkins has
control of this dedicated daemon through its socket, so this job must only build
trusted local repository code. The source bind is deliberately limited to this
checkout, and the SSH agent and whole-home mounts are disabled.

## Cached IBM sources

For IBM semantic development, import and verify only the source scope needed:

```bash
docker/dev import-ibm-cache "$TMPDIR/cobolgrammar/topic-cache" --scope ibm-cics-ts-6x-2026-08-31
docker/dev docs status --scope ibm-cics-ts-6x-2026-08-31
docker/dev docs search "CICS command" --subsystem cics
docker/dev docs read TOPIC_PATH --sha256 DIGEST
```

The development container supplies
`MAINFRAME_ENV_IBM_DOCS_CACHE=/ibm-docs/topic-cache`, and the cache volume is
mounted only by the development service. Topic and TOC bytes must match explicit
pins; the offline reader grants no semantic or licensed-execution credit. When a
pinned HTML source is missing or needs refresh, use the existing Chrome Browser
Control retrieval workflow documented in the runbook. This uses the normal
host-driven development container, not nested Docker or another runner. See the
[IBM documentation cache runbook](../docs/runbooks/IBM-DOCS-CACHE.md).

## CI and deployment

The image uses digest-pinned bases, Rust 1.98.0 plus MSRV 1.95.0 and the locked
fuzz nightly, Python 3.12.13, source-hash-verified Git 2.50.1 and PostgreSQL 18.6,
and exact Cargo tool versions. Jenkins uses the existing WAR/plugin hash lock.
No plugins are installed from a floating update center at startup.
The verified WAR and plugin downloads are cached under the capped volume's
`jenkins-seed` directory and reverified during image builds and controller
startup, so image-cache cleanup does not force another plugin download.

The local pipeline checks formatting, the source-input inventory, `cargo deny`,
the shipped Python/shell tooling tests, MSRV, workspace tests, Clippy, and the
existing disposable PostgreSQL parity suite. Only then does it compile the
release-mode server and deploy the exact binary with its commit and SHA-256
receipt. Jenkins serializes builds and atomically changes the `current` binary
link, restarts the container, and checks the application's reported readiness.
An unhealthy candidate restores and checks the previous binary; a failed first
deployment stops the application. A failed build never reaches deployment.

`CHECK_MODE=auto` narrows prose-only changes relative to the last successful
ancestor commit: dependency/license policy, documentation, and Python/shell
tooling still run, while runtime tests, release compilation, and deployment are
skipped. Missing/unavailable history, normative documents, code/infrastructure,
and unknown inputs retain every local runtime gate. Choose `CHECK_MODE=runtime`
in Jenkins to force those gates. Per-command JSON/logs and timings are archived
under `ci-checks/`; see the [verification workflow](../docs/runbooks/VERIFICATION-WORKFLOW.md)
for scope, limits, and the distinction from root full/release assurance.

Binary rollback does **not** roll back database migrations. Use only compatible
schema changes or follow the [backup and restore runbook](../docs/runbooks/BACKUP-RESTORE.md)
before a migration. The original root `Jenkinsfile` remains the full assurance
and signed-release pipeline; this local Docker pipeline does not claim licensed
IBM evidence, certification, or signed release status.

Older `main` commits have a release-host unit test that panics on Linux ARM,
which is a development platform rather than an advertised release target. The
native test fix is included in this change. Until that fix reaches `main`, the
Docker test runner explicitly filters only that incompatible unit test and
requires the real `runtime-architecture` CLI to reject certification with
`release target is invalid`. Once the old unwrap is gone, all unit tests run
without this substitution. No application tests are excluded.

## Operate

```bash
docker/dev status
docker/dev logs jenkins
docker/dev clean
docker/dev down
docker/dev stop
```

`clean` waits for a development command to finish, clears only development
Cargo/compiler caches, and reclaims unused builder layers/dangling images on
this dedicated daemon. It never prunes volumes or application data. CI reclaims
its own cache after a build when usage reaches 75%. `down` removes containers
and the network while preserving named volumes. `stop` also stops the VM.
Run `init` again after restarting the Mac, then `up`; existing secrets, volumes,
and Jenkins history are retained. Re-run `up` after changing an image recipe or
the baked-in Jenkins pipeline.

Backups exported outside the capped volume consume storage outside this budget.
Do not delete the sparse bundle as a cleanup step: it contains the database,
Jenkins credentials, and deployment state. On Linux, reuse the Compose recipes
with a separately provisioned capped Docker data filesystem; the macOS wrapper
does not silently fall back to an uncapped host daemon.

Upstream references: [Docker storage and bind mounts](https://docs.docker.com/engine/storage/bind-mounts/),
[Colima profiles](https://colima.run/docs/profiles/), and
[Jenkins initialization hooks](https://www.jenkins.io/doc/book/managing/groovy-hook-scripts/).
