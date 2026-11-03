# The image the laptop CI runners run in (ci/runner/compose.yml): GitHub's
# runner on Ubuntu 24.04, the same release as ubuntu-latest, plus what the
# workflows expect the hosted image to have. The toolchain itself comes from
# the workflow steps (dtolnay/rust-toolchain, setup-node, Playwright).
FROM myoung34/github-runner:ubuntu-noble

RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      build-essential pkg-config curl ca-certificates git python3 mold \
 && rm -rf /var/lib/apt/lists/*

# .cargo/config.toml names the Pi cross linker for aarch64-unknown-linux-gnu,
# which is the native target here (arm64 Docker on an Apple-silicon Mac).
# It links with mold: most of an incremental CI build is relinking the
# debug binary and test executables, which GNU ld is slow at.
RUN printf '#!/bin/sh\nexec gcc -fuse-ld=mold "$@"\n' > /usr/local/bin/aarch64-unknown-linux-gnu-gcc \
 && chmod +x /usr/local/bin/aarch64-unknown-linux-gnu-gcc

# Playwright's Chromium and the system libraries it needs, so a new or reset
# runner's first e2e job doesn't spend a minute on apt and the download.
# scripts/ci-runners.sh passes the version from e2e/package.json; when that
# changes, the jobs download the new browser until the next `up`. Node is
# only needed here, for npx; the jobs get theirs from setup-node.
ARG PLAYWRIGHT_VERSION=1.64.1
ARG NODE_VERSION=24.22.0
ENV PLAYWRIGHT_BROWSERS_PATH=/ms-playwright
RUN arch=$(dpkg --print-architecture | sed 's/amd64/x64/') \
 && curl -fsSL "https://nodejs.org/dist/v$NODE_VERSION/node-v$NODE_VERSION-linux-$arch.tar.xz" | tar -xJ -C /tmp \
 && PATH="/tmp/node-v$NODE_VERSION-linux-$arch/bin:$PATH" npx -y "playwright@$PLAYWRIGHT_VERSION" install --with-deps chromium \
 && rm -rf /tmp/node-v* /root/.npm /var/lib/apt/lists/*

ENV CARGO_HOME=/root/.cargo RUSTUP_HOME=/root/.rustup PATH=/root/.cargo/bin:$PATH
RUN curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain none
