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
        choice(name: 'RUN_MODE', choices: ['auto', 'full'],
               description: 'auto selects changed-path gates; full runs all subsystem assurance gates including disposable PostgreSQL 18 parity.')
        string(name: 'JENKINS_VOLUME', defaultValue: '/Volumes/MainframeEnvJenkins',
               description: 'Mounted filesystem with a hard capacity no larger than 10 GiB.')
        string(name: 'BASE_SHA', defaultValue: '',
               description: 'Optional full comparison SHA. Jenkins PR and previous-build metadata are used when empty.')

    }

    environment {
        MAINFRAME_ENV_JENKINS_VOLUME = "${params.JENKINS_VOLUME}"
        CARGO_HOME = "${params.JENKINS_VOLUME}/cargo-home"
        CARGO_TARGET_DIR = "${WORKSPACE}/target"
        TMPDIR = "${WORKSPACE}/target/tmp"
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
                checkout scm
                // Git checkout may clean untracked paths, including the build temp
                // directory when it lives below WORKSPACE. Recreate it afterwards.
                sh 'mkdir -p "$TMPDIR"'
                sh '''#!/bin/bash
                    set -euo pipefail
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
                    command -v cargo-deny >/dev/null || {
                      echo 'cargo-deny is required on the Jenkins node' >&2
                      exit 1
                    }
                    cargo deny --version
                    grep -Eq '^llvm-tools(-|$)' <<<"$components" || {
                      echo "llvm-tools-preview is missing from installed toolchain $required" >&2
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
                    if (params.RUN_MODE == 'full') {
                        env.MAINFRAME_ENV_CI_EVENT = 'manual'
                    } else if (timed) {
                        env.MAINFRAME_ENV_CI_EVENT = 'schedule'
                    } else if (env.CHANGE_ID) {
                        env.MAINFRAME_ENV_CI_EVENT = 'pull_request'
                    } else {
                        env.MAINFRAME_ENV_CI_EVENT = 'push'
                    }

                    if (env.MAINFRAME_ENV_CI_EVENT == 'pull_request') {
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
                    def selector = { field ->
                        sh(
                            script: "\"${env.MAINFRAME_ENV_PYTHON}\" -B tools/jenkins/plan_field.py --plan \"${env.CARGO_TARGET_DIR}/ci-assurance/plan.json\" --field ${field}",
                            returnStdout: true
                        ).trim()
                    }
                    env.CI_BUILD_REQUIRED = selector('build')
                    env.CI_MSRV_REQUIRED = selector('msrv')
                    env.CI_STORE_REQUIRED = selector('store')
                    env.CI_ARCHITECTURE = selector('architecture')
                    env.CI_EVIDENCE = selector('evidence')
                    env.CI_MUTATION = selector('mutation')
                    env.CI_FULL = selector('full')
                    env.CI_DOCS = selector('docs')
                    env.CI_TARGETS = selector('targets')
                    env.CI_DOCUMENTATION = selector('documentation')
                }
            }
        }

        stage('Dependency and license policy') {
            steps {
                sh '''#!/bin/bash
                    set -euo pipefail
                    out="$CARGO_TARGET_DIR/ci-assurance"
                    "$MAINFRAME_ENV_PYTHON" -B tools/ci_assurance.py record \
                      --output "$out" --gate supply-chain -- \
                      "$MAINFRAME_ENV_PYTHON" -B tools/supply_chain.py check \
                        --runtime ci --jenkins-home "$JENKINS_HOME"
                    "$MAINFRAME_ENV_PYTHON" -B tools/ci_assurance.py record \
                      --output "$out" --gate cargo-deny -- cargo deny check
                    "$MAINFRAME_ENV_PYTHON" -B tools/ci_assurance.py record \
                      --output "$out" --gate license-notices -- cargo xtask license-notices --check
                '''
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
                    "$MAINFRAME_ENV_PYTHON" -B tools/ci_assurance.py record --output "$out" --gate python-tooling-tests --expect-tests -- "$MAINFRAME_ENV_PYTHON" -B tools/run_tooling_tests.py
                    "$MAINFRAME_ENV_PYTHON" -B tools/ci_assurance.py record --output "$out" --gate api-docs -- "$MAINFRAME_ENV_PYTHON" -B tools/check_public_api_docs.py
                    # Supply the actual effective spec to the scoped contract-consumption tests.
                    # The output directory is ignored and archived by the existing CI owner.
                    export MAINFRAME_ENV_CONFORMANCE_SPEC_EXPORT="$out/effective-conformance-spec.json"
                    cargo run --quiet --locked -p xtask -- conformance-spec-export > "$MAINFRAME_ENV_CONFORMANCE_SPEC_EXPORT"
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
                    expression { env.CI_DOCS == 'true' }
                }
            }
            steps {
                sh '''#!/bin/bash
                    set -euo pipefail
                    out="$CARGO_TARGET_DIR/ci-assurance"
                    if [[ "$CI_DOCS" == true ]]; then
                      "$MAINFRAME_ENV_PYTHON" -B tools/ci_assurance.py record --output "$out" --gate docs -- cargo xtask docs --check
                    fi
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
                    "$MAINFRAME_ENV_PYTHON" -B tools/ci_assurance.py record --output "$out" --gate runtime-architecture -- cargo xtask runtime-architecture --check
                    "$MAINFRAME_ENV_PYTHON" -B tools/ci_assurance.py record --output "$out" --gate model-check --expect-tests -- tools/run_model_assurance.sh
                    "$MAINFRAME_ENV_PYTHON" -B tools/ci_assurance.py record --output "$out" --gate fuzz-smoke -- tools/run_fuzz_assurance.sh smoke
                    "$MAINFRAME_ENV_PYTHON" -B tools/ci_assurance.py record --output "$out" --gate fuzz-periodic -- tools/run_fuzz_assurance.sh periodic
                    "$MAINFRAME_ENV_PYTHON" -B tools/ci_assurance.py record --output "$out" --gate coverage-baseline -- tools/run_coverage_baseline.sh
                '''
            }
        }

        stage('MSRV') {
            when { expression { env.CI_MSRV_REQUIRED == 'true' } }
            steps {
                sh '''#!/bin/bash
                    set -euo pipefail
                    rustup toolchain list | grep -q '^1\\.95\\.0-' || {
                      echo 'Rust 1.95.0 is required; install it on the Jenkins node before running CI.' >&2
                      exit 1
                    }
                    "$MAINFRAME_ENV_PYTHON" -B tools/ci_assurance.py record \
                      --output "$CARGO_TARGET_DIR/ci-assurance" --gate msrv -- \
                      cargo +1.95.0 check --workspace --all-targets --all-features --locked
                '''
            }
        }

        stage('PostgreSQL parity') {
            when { expression { env.CI_STORE_REQUIRED == 'true' } }
            steps {
                sh 'tools/jenkins/postgres_parity.sh run'
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
                  gates=()
                  while IFS= read -r gate; do gates+=("$gate"); done < <(tools/jenkins/postgres_parity.sh list)
                  "$MAINFRAME_ENV_PYTHON" -B tools/ci_assurance.py summary --plan "$backend/plan.json" \
                    --directory "$backend" --output "$backend/summary.json" \
                    --gates "${gates[@]}"
                fi
            ''')
            archiveArtifacts artifacts: 'target/ci-assurance/**/*,target/ci-backend/**/*,target/coverage/**/*,target/fuzz-artifacts-*/*,target/jenkins-artifacts/**/*,.postgres/postgres.log',
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
