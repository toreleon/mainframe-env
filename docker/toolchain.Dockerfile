FROM docker.io/library/python@sha256:3cd9086bdb30f7c9bc08a3fa621d9842e0d3f6f9291aeb4677e0547817c10b12 AS python
FROM docker.io/jenkins/jenkins@sha256:c1e4c349365f6d16d88595b2c5f7e8ff39b8ae1d061f62420bac193b4b9616d0 AS java
FROM docker.io/library/docker@sha256:51e23845f5caff1e688a2fae003b0c69d635c9200ad544731db1593731df1d3a AS dockercli
FROM docker.io/library/rust@sha256:82150a52ec202c1b14d7817e14516c392bb7f5cfebd88f1ed531cb37ebd39922
COPY --from=python /usr/local/ /usr/local/
COPY --from=java /opt/java/openjdk/ /opt/java/openjdk/
COPY --from=dockercli /usr/local/bin/docker /usr/local/bin/docker
ENV PATH="/opt/java/openjdk/bin:/opt/postgresql/bin:/usr/local/cargo/bin:${PATH}" \
    PYTHONDONTWRITEBYTECODE=1 CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2
COPY docker/install-tools.py docker/inputs.lock.json /opt/mainframe-env/docker/
RUN python3 /opt/mainframe-env/docker/install-tools.py
RUN rustup set auto-self-update disable \
    && rustup component add --toolchain 1.98.0 rustfmt clippy llvm-tools-preview \
    && rustup toolchain install 1.95.0 --profile minimal \
    && rustup toolchain install nightly-2026-09-01 --profile minimal \
    && cargo +1.98.0 install cargo-deny --version 0.20.2 --locked \
    && cargo +1.98.0 install cargo-fuzz --version 0.13.2 --locked \
    && cargo +1.98.0 install cargo-llvm-cov --version 0.9.1 --locked \
    && rm -rf /usr/local/cargo/registry /usr/local/cargo/git /root/.cache
COPY tools/supply_chain.py tools/ci-inputs.lock.json /opt/mainframe-env/tools/
COPY tools/jenkins/controller-plugins.lock.json /opt/mainframe-env/tools/jenkins/
COPY --from=jenkins-seed / /opt/jenkins-seed/
RUN python3 /opt/mainframe-env/tools/supply_chain.py verify-jenkins --home /opt/jenkins-seed
RUN useradd --create-home --uid 1000 developer \
    && mkdir -p /cache /target /state/jenkins /releases \
    && chown 1000:1000 /cache /target /state/jenkins /releases \
    && git config --system --add safe.directory /source \
    && git config --system --add safe.directory /workspace
COPY docker/ /opt/mainframe-env/docker/
COPY docker/codex-container.toml /etc/codex/config.toml
WORKDIR /workspace
CMD ["bash"]
