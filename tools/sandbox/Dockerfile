FROM rust:1.98.0-bookworm@sha256:82150a52ec202c1b14d7817e14516c392bb7f5cfebd88f1ed531cb37ebd39922 AS build
WORKDIR /build
COPY . .
# Keep build and bundle preparation in one layer, including on VFS-based daemons.
RUN --mount=type=secret,id=proxy_ca \
    --mount=type=cache,id=mainframe-sandbox-cargo-registry,target=/usr/local/cargo/registry \
    if [ -f /run/secrets/proxy_ca ]; then \
      export CARGO_HTTP_CAINFO=/run/secrets/proxy_ca CURL_CA_BUNDLE=/run/secrets/proxy_ca GIT_SSL_CAINFO=/run/secrets/proxy_ca; \
    fi; \
    cargo +1.98.0 build --locked --release -p mainframe-env-cli -p mainframe-env-conformance -p xtask \
      --bin mainframe-env --bin mainframe-sandbox-runtime --bin xtask && \
    curl --fail --show-error --location https://www.kernel.org/pub/software/scm/git/git-2.50.1.tar.xz -o /tmp/git.tar.xz && \
    echo '7e3e6c36decbd8f1eedd14d42db6674be03671c2204864befa2a41756c5c8fc4  /tmp/git.tar.xz' | sha256sum --check && \
    tar -xJf /tmp/git.tar.xz -C /tmp && \
    make -C /tmp/git-2.50.1 -j4 prefix=/opt/mainframe-sandbox/git NO_GETTEXT=YesPlease NO_TCLTK=YesPlease install && \
    mkdir -p /out/bin /out/tools /out/share /out/reference && \
    cp target/release/mainframe-env target/release/mainframe-sandbox-runtime bin/mainframe-sandbox /out/bin/ && \
    cp -r tools/sandbox /out/tools/ && \
    cp conformance/profiles/carddemo/inventory/carddemo-corpus.json /out/share/ && \
    cp -r LICENSE NOTICE LICENSES /out/ && \
    cp -a /opt/mainframe-sandbox/git /out/ && \
    cp /tmp/git.tar.xz /out/git/git-source.tar.xz && cp /tmp/git-2.50.1/COPYING /out/git/ && \
    RUSTUP_TOOLCHAIN=1.98.0 target/release/xtask license-notices --output /out/THIRD-PARTY-NOTICES.md && \
    /opt/mainframe-sandbox/git/bin/git clone --no-checkout https://github.com/aws-samples/aws-mainframe-modernization-carddemo.git /out/reference/carddemo && \
    /opt/mainframe-sandbox/git/bin/git -C /out/reference/carddemo checkout --detach 59cc6c2fd7ebd7ef7925cad552a01a4b8b6e4d5e && \
    rm -rf target /tmp/git-2.50.1 /tmp/git.tar.xz

FROM python:3.12.13-bookworm@sha256:3cd9086bdb30f7c9bc08a3fa621d9842e0d3f6f9291aeb4677e0547817c10b12
COPY --from=build /out /opt/mainframe-sandbox
ENV PATH=/opt/mainframe-sandbox/bin:/opt/mainframe-sandbox/git/bin:$PATH
RUN groupadd --gid 10001 sandbox && useradd --uid 10001 --gid sandbox --create-home sandbox && \
    mkdir /state && chown sandbox:sandbox /state && \
    mkdir -p /opt/mainframe-sandbox/git/etc && \
    git config --system --add safe.directory /opt/mainframe-sandbox/reference/carddemo && \
    chmod -R a+rX /opt/mainframe-sandbox
USER sandbox
EXPOSE 8080 49983
HEALTHCHECK --interval=10s --timeout=3s --start-period=60s \
    CMD python -c "import urllib.request; urllib.request.build_opener(urllib.request.ProxyHandler({})).open('http://127.0.0.1:8080/readyz', timeout=2)"
ENTRYPOINT ["mainframe-sandbox"]
CMD ["serve", "--instance", "/state/carddemo", "--listen", "0.0.0.0:8080", "--allow-network-listener"]
