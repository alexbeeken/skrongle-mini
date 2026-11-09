# The release QA image (scripts/release-qa/compose.yaml): a stock Debian of
# the target architecture plus curl, with package-smoke.sh and the
# entrypoint copied in. The same Dockerfile builds every architecture; the
# Compose file sets `platform:` and these arguments per service.
#
#   BASE             the image to start from. Debian 11 (glibc 2.31, the oldest
#                    we support) for aarch64, x86_64 and armv7l; Raspbian 11
#                    for armv6l, since Debian's armhf is armv7 and Alpine is musl.
#   PLATFORM         the image's platform (linux/arm64, linux/amd64, linux/arm/v7,
#                    linux/arm/v6), on the FROM line so that a Docker without
#                    BuildKit (the classic builder, which ignores --platform from
#                    Compose) still pulls the right base and marks the image.
#   ARCHIVE_SOURCES  apt sources to write over the image's own, for a Debian
#                    whose packages moved to archive.debian.org (Debian 11 did
#                    in 2026). Empty keeps the image's sources.
ARG BASE=debian:bullseye-slim
ARG PLATFORM
FROM --platform=${PLATFORM} ${BASE}
ARG ARCHIVE_SOURCES=""
RUN set -eu; \
    if [ -n "$ARCHIVE_SOURCES" ]; then printf '%b\n' "$ARCHIVE_SOURCES" > /etc/apt/sources.list; rm -rf /etc/apt/sources.list.d/*; fi; \
    apt-get update; \
    apt-get install -y --no-install-recommends curl ca-certificates procps; \
    rm -rf /var/lib/apt/lists/*
COPY package-smoke.sh entrypoint.sh /usr/local/lib/release-qa/
RUN chmod 0755 /usr/local/lib/release-qa/*.sh
# Where `serve` unpacks the tarball and the server keeps its database.
RUN mkdir -p /opt/skrongle /tarballs /var/lib/skrongle
VOLUME /var/lib/skrongle
EXPOSE 8080
ENV SKRONGLE_BIND=0.0.0.0:8080 \
    SKRONGLE_DB=/var/lib/skrongle/learn.db \
    SKRONGLE_DATA_DIR=/opt/skrongle/data \
    SKRONGLE_SIGNUP=invite \
    SKRONGLE_LOG=info
ENTRYPOINT ["/usr/local/lib/release-qa/entrypoint.sh"]
CMD ["serve"]
