pipeline {
    agent any

    options {
        skipDefaultCheckout(true)
        disableConcurrentBuilds()
        buildDiscarder(logRotator(numToKeepStr: '5', artifactNumToKeepStr: '1'))
        timeout(time: 180, unit: 'MINUTES')
    }

    triggers {
        pollSCM('H/5 * * * *')
        cron('H 20 * * 0')
    }

    parameters {
        choice(name: 'RUN_MODE', choices: ['auto', 'full', 'release'],
               description: 'auto selects changed-path gates; full runs all assurance gates including disposable PostgreSQL 18 parity; release also verifies artifacts.')
        string(name: 'JENKINS_VOLUME', defaultValue: '/Volumes/MainframeEnvJenkins',
               description: 'Mounted filesystem with a hard capacity no larger than 10 GiB.')
        string(name: 'BASE_SHA', defaultValue: '',
               description: 'Optional full comparison SHA. Jenkins PR and previous-build metadata are used when empty.')
        string(name: 'RELEASE_TAG', defaultValue: '',
               description: 'Existing post-migration mainframe-env-vX.Y.Z tag containing this Jenkins pipeline and its helpers.')
        string(name: 'RELEASE_TARGET', defaultValue: '',
               description: 'Supported target: aarch64-apple-darwin or x86_64-unknown-linux-gnu; empty selects the host only when it is one of those two.')
        booleanParam(name: 'PUBLISH_GITHUB_RELEASE', defaultValue: false,
                     description: 'Explicitly publish the generated offline Cargo bundle to the existing GitHub release.')
        string(name: 'GITHUB_CREDENTIAL_ID', defaultValue: 'mainframe-env-github-token',
               description: 'Jenkins Secret Text credential used only when publication is enabled.')
    }

    environment {
        MAINFRAME_ENV_JENKINS_VOLUME = "${params.JENKINS_VOLUME}"
        CARGO_HOME = "${params.JENKINS_VOLUME}/cargo-home"
        CARGO_TARGET_DIR = "${WORKSPACE}/target"
        TMPDIR = "${WORKSPACE}/.tmp"
        CARGO_INCREMENTAL = '0'
        CARGO_BUILD_JOBS = '2'
        CARGO_PROFILE_DEV_DEBUG = '0'
        CARGO_PROFILE_TEST_DEBUG = '0'
        RUSTUP_AUTO_INSTALL = '0'
        PYTHONDONTWRITEBYTECODE = '1'
    }

    stages {
        stage('Capped storage boundary') {
            steps {
                sh '''#!/bin/sh
                    set -eu
                    test -n "${JENKINS_HOME:-}" || {
                      echo "JENKINS_HOME is not exported by the controller" >&2
                      exit 1
                    }
                    case "$JENKINS_HOME/" in
                      "$MAINFRAME_ENV_JENKINS_VOLUME/"*) ;;
                      *) echo "JENKINS_HOME is outside the capped volume: $JENKINS_HOME" >&2; exit 1 ;;
                    esac
                    case "$WORKSPACE/" in
                      "$MAINFRAME_ENV_JENKINS_VOLUME/"*) ;;
                      *) echo "WORKSPACE is outside the capped volume: $WORKSPACE" >&2; exit 1 ;;
                    esac
                    mkdir -p "$CARGO_HOME" "$CARGO_TARGET_DIR" "$TMPDIR"
                '''
            }
        }

        stage('Checkout') {
            steps {
                deleteDir()
                sh 'mkdir -p "$TMPDIR"'
                checkout scm
                sh '''#!/bin/bash
                    set -euo pipefail
                    if [[ "$RUN_MODE" == release && -n "$RELEASE_TAG" ]]; then
                      [[ "$RELEASE_TAG" =~ ^mainframe-env-v(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)$ ]] || {
                        echo "invalid release tag: $RELEASE_TAG" >&2
                        exit 1
                      }
                      git fetch --quiet --tags --force origin "refs/tags/$RELEASE_TAG:refs/tags/$RELEASE_TAG"
                      required_paths=(
                        Jenkinsfile
                        tools/ci_assurance.py
                        tools/dataset_mutations.py
                        tools/jenkins/disk_guard.py
                        tools/jenkins/postgres_parity.sh
                        tools/package_offline_cargo_bundle.sh
                      )
                      for required_path in "${required_paths[@]}"; do
                        git cat-file -e "$RELEASE_TAG:$required_path" 2>/dev/null || {
                          echo "$RELEASE_TAG predates the Jenkins migration or lacks $required_path" >&2
                          exit 1
                        }
                      done
                      git checkout --quiet --detach "$RELEASE_TAG"
                    fi
                    git rev-parse HEAD
                '''
            }
        }

        stage('Installed toolchain preflight') {
            steps {
                script {
                    env.MAINFRAME_ENV_PYTHON = sh(
                        script: 'tools/jenkins/select-python.sh',
                        returnStdout: true
                    ).trim()
                }
                sh '''#!/bin/bash
                    set -euo pipefail
                    "$MAINFRAME_ENV_PYTHON" -c \
                      'import hashlib, math, ssl; hashlib.sha256(b"jenkins").digest()'
                    required="$(awk -F'"' '/^[[:space:]]*channel[[:space:]]*=/{print $2; exit}' rust-toolchain.toml)"
                    [[ -n "$required" ]] || {
                      echo 'rust-toolchain.toml does not declare a channel' >&2
                      exit 1
                    }
                    for toolchain in "$required" 1.95.0; do
                      RUSTUP_AUTO_INSTALL=0 rustup run "$toolchain" rustc --version >/dev/null
                      RUSTUP_AUTO_INSTALL=0 rustup run "$toolchain" cargo --version >/dev/null
                    done
                    components="$(RUSTUP_AUTO_INSTALL=0 rustup component list \
                      --toolchain "$required" --installed)"
                    grep -Eq '^rustfmt(-|$)' <<<"$components" || {
                      echo "rustfmt is missing from installed toolchain $required" >&2
                      exit 1
                    }
                    grep -Eq '^clippy(-|$)' <<<"$components" || {
                      echo "clippy is missing from installed toolchain $required" >&2
                      exit 1
                    }
                '''
            }
        }

        stage('Verify capped filesystem') {
            steps {
                sh '''#!/bin/bash
                    set -euo pipefail
                    "$MAINFRAME_ENV_PYTHON" -B tools/jenkins/disk_guard.py prune \
                      --root "$MAINFRAME_ENV_JENKINS_VOLUME" \
                      --cargo-home "$CARGO_HOME"
                    "$MAINFRAME_ENV_PYTHON" -B tools/jenkins/disk_guard.py verify \
                      --root "$MAINFRAME_ENV_JENKINS_VOLUME" \
                      --require "JENKINS_HOME=$JENKINS_HOME" \
                      --require "WORKSPACE=$WORKSPACE" \
                      --require "CARGO_HOME=$CARGO_HOME" \
                      --require "CARGO_TARGET_DIR=$CARGO_TARGET_DIR" \
                      --require "TMPDIR=$TMPDIR"
                '''
            }
        }

        stage('Resolve build context') {
            steps {
                script {
                    def timed = !currentBuild.getBuildCauses('hudson.triggers.TimerTrigger$TimerTriggerCause').isEmpty()
                    def tag = params.RELEASE_TAG?.trim()
                    if (!tag && env.TAG_NAME) {
                        tag = env.TAG_NAME
                    }
                    if (!tag && env.BRANCH_NAME?.startsWith('mainframe-env-v')) {
                        tag = env.BRANCH_NAME
                    }

                    if (params.RUN_MODE == 'release' || tag) {
                        env.MAINFRAME_ENV_CI_EVENT = 'tag'
                    } else if (params.RUN_MODE == 'full') {
                        env.MAINFRAME_ENV_CI_EVENT = 'manual'
                    } else if (timed) {
                        env.MAINFRAME_ENV_CI_EVENT = 'schedule'
                    } else if (env.CHANGE_ID) {
                        env.MAINFRAME_ENV_CI_EVENT = 'pull_request'
                    } else {
                        env.MAINFRAME_ENV_CI_EVENT = 'push'
                    }

                    if (env.MAINFRAME_ENV_CI_EVENT == 'tag') {
                        env.MAINFRAME_ENV_RELEASE_TAG = tag ?: ''
                        env.MAINFRAME_ENV_CI_REF = "refs/tags/${env.MAINFRAME_ENV_RELEASE_TAG}"
                    } else if (env.MAINFRAME_ENV_CI_EVENT == 'pull_request') {
                        env.MAINFRAME_ENV_CI_REF = "refs/pull/${env.CHANGE_ID}/merge"
                    } else {
                        env.MAINFRAME_ENV_CI_REF = "refs/heads/${env.BRANCH_NAME ?: 'main'}"
                    }

                    env.MAINFRAME_ENV_CI_BASE = params.BASE_SHA?.trim() ?: ''
                    if (!env.MAINFRAME_ENV_CI_BASE && env.CHANGE_TARGET) {
                        env.MAINFRAME_ENV_CI_BASE = sh(
                            script: 'git rev-parse --verify "refs/remotes/origin/${CHANGE_TARGET}^{commit}"',
                            returnStdout: true
                        ).trim()
                    }
                    if (!env.MAINFRAME_ENV_CI_BASE && env.GIT_PREVIOUS_COMMIT ==~ /[0-9a-f]{40}/) {
                        env.MAINFRAME_ENV_CI_BASE = env.GIT_PREVIOUS_COMMIT
                    }
                    env.MAINFRAME_ENV_MERGE_COMMIT = sh(
                        script: "git log -1 --pretty=%s | grep -q '^Merge pull request #'",
                        returnStatus: true
                    ) == 0 ? 'true' : 'false'
                }
            }
        }

        stage('Plan assurance') {
            steps {
                sh '''#!/bin/bash
                    set -euo pipefail
                    "$MAINFRAME_ENV_PYTHON" -B -m unittest discover -s tools/tests -p 'test_ci_assurance.py'
                    "$MAINFRAME_ENV_PYTHON" -B -m unittest discover -s tools/tests -p 'test_jenkins_disk_guard.py'
                    merge_flag=''
                    [[ "$MAINFRAME_ENV_MERGE_COMMIT" == true ]] && merge_flag='--merge-commit'
                    "$MAINFRAME_ENV_PYTHON" -B tools/ci_assurance.py plan \
                      --event "$MAINFRAME_ENV_CI_EVENT" \
                      --ref "$MAINFRAME_ENV_CI_REF" \
                      --base "${MAINFRAME_ENV_CI_BASE:-}" \
                      --output "$CARGO_TARGET_DIR/ci-assurance/plan.json" \
                      ${merge_flag:+$merge_flag}
                '''
                script {
                    def plan = new groovy.json.JsonSlurperClassic().parseText(
                        readFile("${env.CARGO_TARGET_DIR}/ci-assurance/plan.json")
                    )
                    env.CI_BUILD_REQUIRED = plan.build.toString()
                    env.CI_MSRV_REQUIRED = plan.msrv.toString()
                    env.CI_STORE_REQUIRED = plan.store.toString()
                    env.CI_ARCHITECTURE = plan.architecture.toString()
                    env.CI_EVIDENCE = plan.evidence.toString()
                    env.CI_MUTATION = plan.mutation.toString()
                    env.CI_FULL = plan.full.toString()
                    env.CI_TARGETS = plan.primary_gates.contains('targets').toString()
                    env.CI_DOCUMENTATION = plan.primary_gates.contains('documentation').toString()
                }
            }
        }

        stage('Foundation') {
            when { expression { env.CI_BUILD_REQUIRED == 'true' } }
            steps {
                sh '''#!/bin/bash
                    set -euo pipefail
                    out="$CARGO_TARGET_DIR/ci-assurance"
                    "$MAINFRAME_ENV_PYTHON" -B tools/ci_assurance.py record --output "$out" --gate fmt -- cargo fmt --all -- --check
                    "$MAINFRAME_ENV_PYTHON" -B tools/ci_assurance.py record --output "$out" --gate spec -- cargo xtask spec --check
                    "$MAINFRAME_ENV_PYTHON" -B tools/ci_assurance.py record --output "$out" --gate cobol -- cargo xtask cobol-exit --check
                    "$MAINFRAME_ENV_PYTHON" -B tools/ci_assurance.py record --output "$out" --gate tests --expect-tests -- cargo test --workspace --all-features --locked --no-fail-fast
                    "$MAINFRAME_ENV_PYTHON" -B tools/ci_assurance.py record --output "$out" --gate clippy -- cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
                '''
            }
        }

        stage('Workspace targets and documentation') {
            when {
                anyOf {
                    expression { env.CI_TARGETS == 'true' }
                    expression { env.CI_DOCUMENTATION == 'true' }
                }
            }
            steps {
                sh '''#!/bin/bash
                    set -euo pipefail
                    out="$CARGO_TARGET_DIR/ci-assurance"
                    if [[ "$CI_TARGETS" == true ]]; then
                      "$MAINFRAME_ENV_PYTHON" -B tools/ci_assurance.py record --output "$out" --gate targets -- cargo check --workspace --all-targets --all-features --locked
                    fi
                    if [[ "$CI_DOCUMENTATION" == true ]]; then
                      "$MAINFRAME_ENV_PYTHON" -B tools/ci_assurance.py record --output "$out" --gate documentation -- env RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps --locked
                    fi
                '''
            }
        }

        stage('Selected fast assurance') {
            when {
                anyOf {
                    expression { env.CI_ARCHITECTURE == 'true' && env.CI_FULL != 'true' }
                    expression { env.CI_EVIDENCE == 'true' && env.CI_FULL != 'true' }
                    expression { env.CI_MUTATION == 'true' }
                }
            }
            steps {
                sh '''#!/bin/bash
                    set -euo pipefail
                    out="$CARGO_TARGET_DIR/ci-assurance"
                    if [[ "$CI_ARCHITECTURE" == true && "$CI_FULL" != true ]]; then
                      "$MAINFRAME_ENV_PYTHON" -B tools/ci_assurance.py record --output "$out" --gate architecture-fast -- cargo xtask architecture-fast --check
                    fi
                    if [[ "$CI_EVIDENCE" == true && "$CI_FULL" != true ]]; then
                      "$MAINFRAME_ENV_PYTHON" -B tools/ci_assurance.py record --output "$out" --gate evidence-fast -- cargo xtask evidence-fast --check
                    fi
                    if [[ "$CI_MUTATION" == true ]]; then
                      "$MAINFRAME_ENV_PYTHON" -B tools/ci_assurance.py record --output "$out" --gate mutation -- "$MAINFRAME_ENV_PYTHON" -B tools/dataset_mutations.py --output "$out/dataset-mutations" --timeout 300
                    fi
                '''
            }
        }

        stage('Full assurance') {
            when { expression { env.CI_FULL == 'true' } }
            steps {
                sh '''#!/bin/bash
                    set -euo pipefail
                    out="$CARGO_TARGET_DIR/ci-assurance"
                    "$MAINFRAME_ENV_PYTHON" -B tools/ci_assurance.py record --output "$out" --gate conformance -- cargo xtask conformance --check
                    "$MAINFRAME_ENV_PYTHON" -B tools/ci_assurance.py record --output "$out" --gate certification -- cargo xtask certification
                    "$MAINFRAME_ENV_PYTHON" -B tools/ci_assurance.py record --output "$out" --gate evidence-seal -- cargo xtask evidence seal --check
                    "$MAINFRAME_ENV_PYTHON" -B tools/ci_assurance.py record --output "$out" --gate runtime-architecture -- cargo xtask runtime-architecture --check
                '''
            }
        }

        stage('Contract MSRV') {
            when { expression { env.CI_MSRV_REQUIRED == 'true' } }
            steps {
                sh '''#!/bin/bash
                    set -euo pipefail
                    rustup toolchain list | grep -q '^1\\.95\\.0-' || {
                      echo 'Rust 1.95.0 is required; install it on the Jenkins node before running CI.' >&2
                      exit 1
                    }
                    cargo +1.95.0 check --locked \
                      -p mainframe-env-source \
                      -p mainframe-env-diagnostics \
                      -p mainframe-env-encoding \
                      -p mainframe-env-ir \
                      -p mainframe-env-compiler-api \
                      -p mainframe-env-execution-api \
                      -p mainframe-env-host-api \
                      -p mainframe-env-store-api
                '''
            }
        }

        stage('PostgreSQL parity') {
            when { expression { env.CI_STORE_REQUIRED == 'true' } }
            steps {
                sh 'tools/jenkins/postgres_parity.sh run'
            }
        }

        stage('Release verification and offline Cargo bundle') {
            when { expression { env.MAINFRAME_ENV_CI_EVENT == 'tag' } }
            steps {
                sh '''#!/bin/bash
                    set -euo pipefail
                    tag="${MAINFRAME_ENV_RELEASE_TAG:-}"
                    [[ "$tag" =~ ^mainframe-env-v(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)$ ]] || {
                      echo "release mode requires an existing mainframe-env-vX.Y.Z tag" >&2
                      exit 1
                    }
                    version="${tag#mainframe-env-v}"
                    [[ "$(tr -d '[:space:]' < VERSION)" == "$version" ]] || {
                      echo "VERSION does not match $tag" >&2
                      exit 1
                    }
                    [[ "$(git rev-parse HEAD)" == "$(git rev-parse --verify "refs/tags/$tag^{commit}")" ]] || {
                      echo "HEAD is not the release tag commit" >&2
                      exit 1
                    }
                    target="$RELEASE_TARGET"
                    [[ -n "$target" ]] || target="$(rustc -vV | awk '/^host: /{print $2}')"
                    case "$target" in
                      aarch64-apple-darwin|x86_64-unknown-linux-gnu) ;;
                      *) echo "unsupported release target: $target" >&2; exit 1 ;;
                    esac
                    cargo xtask release --target "$target"
                    git diff --exit-code -- "release/$version/targets/$target"
                    cargo xtask release --check --target "$target"
                    tools/package_offline_cargo_bundle.sh --tag "$tag" --out "$CARGO_TARGET_DIR/jenkins-artifacts"
                '''
            }
        }

        stage('Publish existing GitHub release') {
            when {
                allOf {
                    expression { env.MAINFRAME_ENV_CI_EVENT == 'tag' }
                    expression { params.PUBLISH_GITHUB_RELEASE }
                }
            }
            steps {
                withCredentials([string(credentialsId: params.GITHUB_CREDENTIAL_ID, variable: 'GH_TOKEN')]) {
                    sh '''#!/bin/bash
                        set -euo pipefail
                        command -v gh >/dev/null || { echo 'gh is required for publication' >&2; exit 1; }
                        tag="${MAINFRAME_ENV_RELEASE_TAG:-}"
                        version="${tag#mainframe-env-v}"
                        archive="$CARGO_TARGET_DIR/jenkins-artifacts/mainframe-env-${version}-cargo-vendor.tar.gz"
                        if ! gh release view "$tag" >/dev/null 2>&1; then
                          gh release create "$tag" --verify-tag --title "mainframe-env $version" --generate-notes
                        fi
                        gh release upload "$tag" "$archive" "$archive.sha256" --clobber
                    '''
                }
            }
        }

        stage('Assurance summary') {
            steps {
                sh '''#!/bin/bash
                    set -euo pipefail
                    out="$CARGO_TARGET_DIR/ci-assurance"
                    "$MAINFRAME_ENV_PYTHON" -B tools/ci_assurance.py summary --plan "$out/plan.json" \
                      --directory "$out" --output "$out/summary.json"
                '''
            }
        }
    }

    post {
        always {
            sh(returnStatus: true, script: 'tools/jenkins/postgres_parity.sh cleanup')
            sh(returnStatus: true, script: '''#!/bin/bash
                set -u
                main="$CARGO_TARGET_DIR/ci-assurance"
                if [[ -f "$main/plan.json" ]]; then
                  "$MAINFRAME_ENV_PYTHON" -B tools/ci_assurance.py summary --plan "$main/plan.json" \
                    --directory "$main" --output "$main/summary.json"
                fi
                backend="$CARGO_TARGET_DIR/ci-backend"
                if [[ -f "$backend/plan.json" ]]; then
                  gates=(postgres-move)
                  [[ -f "$backend/postgres-effect.json" ]] && gates+=(postgres-effect)
                  "$MAINFRAME_ENV_PYTHON" -B tools/ci_assurance.py summary --plan "$backend/plan.json" \
                    --directory "$backend" --output "$backend/summary.json" \
                    --gates "${gates[@]}"
                fi
            ''')
            archiveArtifacts artifacts: 'target/ci-assurance/**/*,target/ci-backend/**/*,target/jenkins-artifacts/**/*,.postgres/postgres.log,release/*/targets/**/*',
                             allowEmptyArchive: true, fingerprint: false
            script {
                if (env.CARGO_TARGET_DIR) {
                    dir(env.CARGO_TARGET_DIR) { deleteDir() }
                }
            }
            sh(returnStatus: true, script: '''#!/bin/bash
                "$MAINFRAME_ENV_PYTHON" -B tools/jenkins/disk_guard.py prune \
                  --root "$MAINFRAME_ENV_JENKINS_VOLUME" \
                  --cargo-home "$CARGO_HOME"
            ''')
            deleteDir()
        }
    }
}
