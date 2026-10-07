FROM rust:1.98.0-bookworm@sha256:82150a52ec202c1b14d7817e14516c392bb7f5cfebd88f1ed531cb37ebd39922 AS build
# Read only Rust/build inputs: controller-only edits reuse the compiled binaries.
# Bind mounts avoid redundant source layers on VFS-based daemons.
RUN --mount=type=bind,source=Cargo.toml,target=/build/Cargo.toml \
    --mount=type=bind,source=Cargo.lock,target=/build/Cargo.lock \
    --mount=type=bind,source=rust-toolchain.toml,target=/build/rust-toolchain.toml \
    --mount=type=bind,source=crates,target=/build/crates \
    --mount=type=bind,source=xtask,target=/build/xtask \
    --mount=type=bind,source=conformance,target=/build/conformance \
    --mount=type=bind,source=docs/documentation-registry.json,target=/build/docs/documentation-registry.json \
    --mount=type=bind,source=LICENSES,target=/build/LICENSES \
    --mount=type=bind,source=LICENSE,target=/build/LICENSE \
    --mount=type=bind,source=NOTICE,target=/build/NOTICE \
    --mount=type=secret,id=proxy_ca \
    --mount=type=cache,id=mainframe-sandbox-cargo-registry,target=/usr/local/cargo/registry \
    set -eu; \
    if [ -f /run/secrets/proxy_ca ]; then \
      export CARGO_HTTP_CAINFO=/run/secrets/proxy_ca CURL_CA_BUNDLE=/run/secrets/proxy_ca GIT_SSL_CAINFO=/run/secrets/proxy_ca; \
    fi; \
    cd /build; \
    cargo +1.98.0 build --locked --release -p mainframe-env-cli -p mainframe-env-conformance -p xtask \
      --bin mainframe-env --bin mainframe-sandbox-runtime --bin xtask; \
    mkdir -p /out/bin; \
    cp target/release/mainframe-env target/release/mainframe-sandbox-runtime /out/bin/; \
    RUSTUP_TOOLCHAIN=1.98.0 target/release/xtask license-notices --output /out/THIRD-PARTY-NOTICES.md; \
    cargo +1.98.0 clean

FROM rust:1.98.0-bookworm@sha256:82150a52ec202c1b14d7817e14516c392bb7f5cfebd88f1ed531cb37ebd39922 AS git-build
RUN --mount=type=secret,id=proxy_ca \
    set -eu; \
    if [ -f /run/secrets/proxy_ca ]; then \
      export CURL_CA_BUNDLE=/run/secrets/proxy_ca GIT_SSL_CAINFO=/run/secrets/proxy_ca; \
    fi; \
    curl --fail --show-error --location https://www.kernel.org/pub/software/scm/git/git-2.50.1.tar.xz -o /tmp/git.tar.xz; \
    echo '7e3e6c36decbd8f1eedd14d42db6674be03671c2204864befa2a41756c5c8fc4  /tmp/git.tar.xz' | sha256sum --check; \
    tar -xJf /tmp/git.tar.xz -C /tmp; \
    make -C /tmp/git-2.50.1 -j4 prefix=/opt/mainframe-sandbox/git CFLAGS=-O2 LDFLAGS=-s NO_GETTEXT=YesPlease NO_TCLTK=YesPlease install; \
    mkdir -p /out/git/lib /out/git/runtime-licenses /out/reference; \
    cp -a /opt/mainframe-sandbox/git/. /out/git/; \
    cp /tmp/git.tar.xz /out/git/git-source.tar.xz; \
    cp /tmp/git-2.50.1/COPYING /out/git/; \
    find /opt/mainframe-sandbox/git/bin /opt/mainframe-sandbox/git/libexec/git-core -type f -executable -printf '%i %p\n' | sort -u -k1,1 | cut -d' ' -f2- > /tmp/git-executables; \
    while read -r executable; do ldd "$executable" 2>/dev/null || true; done < /tmp/git-executables | \
      awk '$3 ~ /^\// {print $3}' | sort -u > /tmp/git-libraries; \
    while read -r library; do \
      case "$(basename "$library")" in libc.so.6|libresolv.so.2|ld-linux*) continue ;; esac; \
      cp -L "$library" /out/git/lib/; \
      dependency_package=$( (dpkg-query -S "$library" 2>/dev/null || dpkg-query -S "$(readlink -f "$library")") | sed 's/: \/.*//' | head -n1); \
      dpkg-query -W -f='${binary:Package}\t${Version}\t${source:Package}\t${source:Version}\n' "$dependency_package" >> /tmp/git-packages; \
      cp -L "/usr/share/doc/${dependency_package%:*}/copyright" "/out/git/runtime-licenses/$dependency_package.txt"; \
    done < /tmp/git-libraries; \
    sort -u /tmp/git-packages > /out/git/RUNTIME-PACKAGES.tsv; \
    rm /out/git/bin/git; \
    printf '%s\n' '#!/bin/sh' 'export LD_LIBRARY_PATH=/opt/mainframe-sandbox/git/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}' \
      'exec /opt/mainframe-sandbox/git/libexec/git-core/git "$@"' > /out/git/bin/git; \
    chmod 755 /out/git/bin/git; \
    /opt/mainframe-sandbox/git/bin/git init --quiet /out/reference/carddemo; \
    /opt/mainframe-sandbox/git/bin/git -C /out/reference/carddemo remote add origin https://github.com/aws-samples/aws-mainframe-modernization-carddemo.git; \
    /opt/mainframe-sandbox/git/bin/git -C /out/reference/carddemo fetch --quiet --depth=1 --no-tags origin 59cc6c2fd7ebd7ef7925cad552a01a4b8b6e4d5e; \
    /opt/mainframe-sandbox/git/bin/git -C /out/reference/carddemo checkout --quiet --detach 59cc6c2fd7ebd7ef7925cad552a01a4b8b6e4d5e; \
    rm -rf /tmp/git-2.50.1 /tmp/git.tar.xz /tmp/git-libraries /tmp/git-packages /tmp/git-executables

FROM scratch AS bundle
COPY --from=build /out /opt/mainframe-sandbox
COPY --from=git-build /out /opt/mainframe-sandbox
# CardDemo verifies ZIP metadata with genuine unzip from the pinned builder.
COPY --from=git-build /usr/bin/unzip /opt/mainframe-sandbox/archive-tools/bin/unzip-real
COPY --from=git-build /usr/lib/*/libbz2.so.1.0.* /opt/mainframe-sandbox/archive-tools/lib/
COPY --from=git-build /usr/share/doc/unzip/copyright /opt/mainframe-sandbox/archive-tools/licenses/unzip.txt
COPY --from=git-build /usr/share/doc/libbz2-1.0/copyright /opt/mainframe-sandbox/archive-tools/licenses/libbz2.txt
COPY tools/sandbox /opt/mainframe-sandbox/tools/sandbox
COPY bin/mainframe-sandbox /opt/mainframe-sandbox/bin/mainframe-sandbox
COPY conformance/profiles/carddemo/inventory/carddemo-corpus.json /opt/mainframe-sandbox/share/carddemo-corpus.json
COPY LICENSE NOTICE /opt/mainframe-sandbox/
COPY LICENSES /opt/mainframe-sandbox/LICENSES

FROM python:3.12.13-slim-bookworm@sha256:4766d8b510c428e595d74b9cc5bbb2fae8e26316fffb4adc89908d79aacd58a2
COPY --from=bundle /opt/mainframe-sandbox /opt/mainframe-sandbox
ENV PATH=/opt/mainframe-sandbox/bin:/opt/mainframe-sandbox/git/bin:$PATH
RUN groupadd --gid 10001 sandbox && useradd --uid 10001 --gid sandbox --create-home sandbox && \
    mkdir /state && chown sandbox:sandbox /state && \
    mkdir -p /opt/mainframe-sandbox/git/etc && \
    git config --system --add safe.directory /opt/mainframe-sandbox/reference/carddemo && \
    ln -s "$(basename /opt/mainframe-sandbox/archive-tools/lib/libbz2.so.1.0.*)" /opt/mainframe-sandbox/archive-tools/lib/libbz2.so.1.0 && \
    printf '%s\n' '#!/bin/sh' 'export LD_LIBRARY_PATH=/opt/mainframe-sandbox/archive-tools/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}' \
      'exec /opt/mainframe-sandbox/archive-tools/bin/unzip-real "$@"' > /opt/mainframe-sandbox/bin/unzip && \
    chmod 755 /opt/mainframe-sandbox/bin/unzip && \
    chmod -R a+rX /opt/mainframe-sandbox
USER sandbox
EXPOSE 8080 49983
HEALTHCHECK --interval=10s --timeout=3s --start-period=60s \
    CMD python -c "import urllib.request; urllib.request.build_opener(urllib.request.ProxyHandler({})).open('http://127.0.0.1:8080/readyz', timeout=2)"
ENTRYPOINT ["mainframe-sandbox"]
CMD ["serve", "--instance", "/state/carddemo", "--listen", "0.0.0.0:8080", "--allow-network-listener"]
