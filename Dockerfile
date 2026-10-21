# ==============================================================================
# Stage 1: SvelteKit Frontend Web Builder
# ==============================================================================
FROM node:22-bookworm-slim AS web-builder

WORKDIR /app/apps/web

# Cache dependency installation layer
COPY apps/web/package.json apps/web/package-lock.json ./
RUN npm ci --no-audit --no-fund

# Copy web application sources and configuration
COPY apps/web/ ./

# Build pre-rendered static frontend and externalize bootstrap scripts for strict CSP
RUN npm run build

# ==============================================================================
# Stage 2: Rust Workspace Builder
# ==============================================================================
FROM rust:slim-bookworm AS rust-builder

ARG GIT_BRANCH=develop
ARG MEDIA_VERSION=""
ENV GIT_BRANCH=${GIT_BRANCH}
ENV MEDIA_VERSION=${MEDIA_VERSION}

RUN apt-get update && apt-get install -y --no-install-recommends pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Copy workspace manifests
COPY Cargo.toml Cargo.lock ./
COPY crates/shared/Cargo.toml ./crates/shared/
COPY crates/node/Cargo.toml ./crates/node/
COPY crates/core/Cargo.toml ./crates/core/
COPY crates/extension-sdk/ ./crates/extension-sdk/
COPY tests/fixtures/extension-sdk/ ./tests/fixtures/extension-sdk/

# Scaffold dummy source hierarchy for dependency precompilation caching
# Explicit test/bin targets declared in crates/node/Cargo.toml and crates/core/Cargo.toml must exist
RUN mkdir -p crates/shared/src crates/node/src crates/node/src/bin crates/core/src crates/core/src/bin crates/core/tests && \
    echo 'pub fn dummy() {}' > crates/shared/src/lib.rs && \
    echo 'fn main() { println!("cargo:rustc-env=AETHER_APP_VERSION=0.1.0"); }' > crates/shared/build.rs && \
    echo 'pub fn dummy() {}' > crates/node/src/lib.rs && \
    echo 'fn main() {}' > crates/node/src/bin/aether-node.rs && \
    echo 'pub fn dummy() {}' > crates/core/src/lib.rs && \
    echo 'fn main() {}' > crates/core/src/main.rs && \
    echo 'fn main() {}' > crates/core/src/bin/aether-core.rs && \
    echo 'fn main() {}' > crates/core/src/bootstrap.rs && \
    echo 'fn main() {}' > crates/core/src/bin/backup.rs && \
    echo 'fn main() {}' > crates/core/src/bin/restore.rs && \
    echo 'fn main() {}' > crates/core/src/bin/recover.rs && \
    echo 'fn main() {}' > crates/core/src/bin/extension-worker.rs && \
    echo 'fn main() {}' > crates/core/src/bin/extension-pack.rs && \
    touch crates/core/tests/p0.rs crates/core/tests/catalog.rs

# Precompile workspace dependencies
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    cargo build --release --locked \
        --bin media-poc \
        --bin aether-core \
        --bin aether-node \
        --bin media-bootstrap \
        --bin media-backup \
        --bin media-restore \
        --bin media-recover \
        --bin media-extension-worker \
        --bin media-extension-pack

# Copy actual workspace crates (shared contracts, node, and core with migrations)
COPY crates/ ./crates/

# Clean dummy binary/deps artifacts, recompile actual release binaries, and strip symbols
RUN touch crates/shared/src/lib.rs \
          crates/shared/build.rs \
          crates/node/src/lib.rs \
          crates/node/src/bin/aether-node.rs \
          crates/core/src/lib.rs \
          crates/core/src/main.rs \
          crates/core/src/bin/aether-core.rs \
          crates/core/src/bootstrap.rs \
          crates/core/src/bin/backup.rs \
          crates/core/src/bin/restore.rs \
          crates/core/src/bin/recover.rs \
          crates/core/src/bin/extension-worker.rs \
          crates/core/src/bin/extension-pack.rs && \
    rm -f target/release/deps/*media_contracts* \
          target/release/deps/*media_node* \
          target/release/deps/*aether_node* \
          target/release/deps/*media_poc* \
          target/release/deps/*aether_core* \
          target/release/deps/*media_bootstrap* \
          target/release/deps/*media_backup* \
          target/release/deps/*media_restore* \
          target/release/deps/*media_recover* \
          target/release/media-poc \
          target/release/aether-core \
          target/release/aether-node \
          target/release/media-bootstrap \
          target/release/media-backup \
          target/release/media-restore \
          target/release/media-recover && \
    cargo build --release --locked \
        --bin media-poc \
        --bin aether-core \
        --bin aether-node \
        --bin media-bootstrap \
        --bin media-backup \
        --bin media-restore \
        --bin media-recover \
        --bin media-extension-worker \
        --bin media-extension-pack && \
    strip --strip-all \
        target/release/media-poc \
        target/release/aether-core \
        target/release/aether-node \
        target/release/media-bootstrap \
        target/release/media-backup \
        target/release/media-restore \
        target/release/media-recover \
        target/release/media-extension-worker \
        target/release/media-extension-pack

# FFmpeg/ffprobe 9.0.2: same verified source as CI, packet-copy runtime profile.
FROM debian:bookworm-slim AS probe-runtime
RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential ca-certificates curl pkg-config nasm xz-utils \
    && rm -rf /var/lib/apt/lists/*
COPY scripts/build-ffmpeg.sh /tmp/build-ffmpeg.sh
RUN bash /tmp/build-ffmpeg.sh /probe runtime

# ==============================================================================
# PostgreSQL client tools for offline and Web backups, shared by Core/standalone.
# The PGDG repository is authenticated with its scoped signed-by key; no DB server.
# ==============================================================================
FROM debian:bookworm-slim AS backup-runtime

# Bound glibc allocator arenas independently of the host CPU count. This limits
# per-thread heap retention after scans/Argon2; it is not an RSS or memory quota.
ENV MALLOC_ARENA_MAX=2

RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates \
        curl \
        tini \
    && curl -fsSL --retry 3 https://www.postgresql.org/media/keys/ACCC4CF8.asc -o /usr/share/keyrings/postgresql-pgdg.asc \
    && printf '%s\n' 'deb [signed-by=/usr/share/keyrings/postgresql-pgdg.asc] https://apt.postgresql.org/pub/repos/apt bookworm-pgdg main' > /etc/apt/sources.list.d/pgdg.list \
    && apt-get update \
    && apt-get install -y --no-install-recommends postgresql-client-18 \
    && pg_dump --version \
    && pg_restore --version \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/* /tmp/* /var/tmp/* /usr/share/man /usr/share/doc

# ==============================================================================
# Stage 3: Distributed Core Runtime (`aether-core`)
# ==============================================================================
FROM backup-runtime AS runtime-core

# Core does not mount or probe media files directly; no ffmpeg needed in Core-only image
RUN groupadd --gid 10001 aether \
    && useradd --uid 10001 --gid 10001 --create-home --home-dir /app --shell /usr/sbin/nologin aether

RUN mkdir -p /data /backups /app/apps/web/build \
    && chown -R aether:aether /data /backups /app \
    && chmod 0750 /data \
    && chmod 0700 /backups \
    && chmod 0755 /app

COPY --from=rust-builder --chmod=0755 /build/target/release/aether-core /usr/local/bin/aether-core
COPY --from=rust-builder --chmod=0755 /build/target/release/media-bootstrap /usr/local/bin/media-bootstrap
COPY --from=rust-builder --chmod=0755 /build/target/release/media-backup /usr/local/bin/media-backup
COPY --from=rust-builder --chmod=0755 /build/target/release/media-restore /usr/local/bin/media-restore
COPY --from=rust-builder --chmod=0755 /build/target/release/media-recover /usr/local/bin/media-recover
COPY --from=rust-builder --chmod=0755 /build/target/release/media-extension-worker /usr/local/bin/media-extension-worker
COPY --from=web-builder --chown=aether:aether /app/apps/web/build /app/apps/web/build

ENV MEDIA_MODE=distributed \
    MEDIA_PORT=3000 \
    MEDIA_BIND_ADDR=0.0.0.0 \
    MEDIA_DB_PATH=/data/core.sqlite3 \
    MEDIA_BACKUP_DIR=/backups \
    MEDIA_WEB_DIST=/app/apps/web/build \
    MEDIA_PUBLIC_URL=http://127.0.0.1:3000

WORKDIR /app
VOLUME ["/data", "/backups"]
EXPOSE 3000
USER aether:aether
STOPSIGNAL SIGINT

HEALTHCHECK --interval=15s --timeout=5s --start-period=5s --retries=3 \
    CMD curl -f http://127.0.0.1:${MEDIA_PORT:-3000}/internal/v1/health || exit 1

ENTRYPOINT ["tini", "--"]
CMD ["aether-core"]

# ==============================================================================
# Stage 4: Distributed Media Node Runtime (`aether-node`)
# ==============================================================================
FROM debian:bookworm-slim AS runtime-node

ENV MALLOC_ARENA_MAX=2

# Node performs ffprobe inspection, NFO/image validation, and confined HTTP Range streaming
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates \
        curl \
        tini \
    && rm -rf /var/lib/apt/lists/*

COPY --from=probe-runtime --chmod=0755 /probe/bin/ffmpeg /usr/local/bin/ffmpeg
COPY --from=probe-runtime --chmod=0755 /probe/bin/ffprobe /usr/local/bin/ffprobe
RUN ln -s /usr/local/bin/ffmpeg /usr/bin/ffmpeg \
    && ln -s /usr/local/bin/ffprobe /usr/bin/ffprobe
COPY --from=probe-runtime /probe/share/media-probe /usr/share/media-probe

RUN groupadd --gid 10001 aether \
    && useradd --uid 10001 --gid 10001 --create-home --home-dir /app --shell /usr/sbin/nologin aether

# /data: Persistent technical state for Node identity & operational inventory (outside /media)
# /media: Mount root for read-only media libraries (:ro)
RUN mkdir -p /data /media /app \
    && chown -R aether:aether /data /media /app \
    && chmod 0750 /data \
    && chmod 0755 /media /app

COPY --from=rust-builder --chmod=0755 /build/target/release/aether-node /usr/local/bin/aether-node

ENV MEDIA_NODE_PORT=4000 \
    MEDIA_NODE_BIND_ADDR=0.0.0.0 \
    MEDIA_NODE_DATA_DIR=/data \
    MEDIA_NODE_ROOTS=/media

WORKDIR /app
VOLUME ["/data"]
EXPOSE 4000
USER aether:aether
STOPSIGNAL SIGINT

HEALTHCHECK --interval=15s --timeout=5s --start-period=5s --retries=3 \
    CMD curl -f http://127.0.0.1:${MEDIA_NODE_PORT:-4000}/internal/v1/health || exit 1

ENTRYPOINT ["tini", "--"]
CMD ["aether-node"]

# ==============================================================================
# Stage 5: Standalone All-in-One Runtime (`aether` — Default Target)
# ==============================================================================
FROM backup-runtime AS runtime

COPY --from=probe-runtime --chmod=0755 /probe/bin/ffmpeg /usr/local/bin/ffmpeg
COPY --from=probe-runtime --chmod=0755 /probe/bin/ffprobe /usr/local/bin/ffprobe
RUN ln -s /usr/local/bin/ffmpeg /usr/bin/ffmpeg \
    && ln -s /usr/local/bin/ffprobe /usr/bin/ffprobe
COPY --from=probe-runtime /probe/share/media-probe /usr/share/media-probe

RUN groupadd --gid 10001 aether \
    && useradd --uid 10001 --gid 10001 --create-home --home-dir /app --shell /usr/sbin/nologin aether

RUN mkdir -p /data /backups /media /app/apps/web/build \
    && chown -R aether:aether /data /backups /media /app \
    && chmod 0750 /data \
    && chmod 0700 /backups \
    && chmod 0755 /media /app

COPY --from=rust-builder --chmod=0755 /build/target/release/media-extension-worker /usr/local/bin/media-extension-worker
COPY --from=rust-builder --chmod=0755 /build/target/release/aether-core /usr/local/bin/aether-core
COPY --from=rust-builder --chmod=0755 /build/target/release/aether-node /usr/local/bin/aether-node
COPY --from=rust-builder --chmod=0755 /build/target/release/media-bootstrap /usr/local/bin/media-bootstrap
COPY --from=rust-builder --chmod=0755 /build/target/release/media-backup /usr/local/bin/media-backup
COPY --from=rust-builder --chmod=0755 /build/target/release/media-restore /usr/local/bin/media-restore
COPY --from=rust-builder --chmod=0755 /build/target/release/media-recover /usr/local/bin/media-recover
# The production standalone server is the same Core with its local Node.
# Keep historical command names without shipping a second full Core executable.
# The fixture-bench media-poc binary remains available in source builds only.
RUN ln -s /usr/local/bin/aether-core /usr/local/bin/media-poc \
    && ln -s /usr/local/bin/aether-core /usr/local/bin/aether

COPY --from=web-builder --chown=aether:aether /app/apps/web/build /app/apps/web/build

ENV MEDIA_PORT=3000 \
    MEDIA_BIND_ADDR=0.0.0.0 \
    MEDIA_DB_PATH=/data/core.sqlite3 \
    MEDIA_BACKUP_DIR=/backups \
    MEDIA_LIBRARY_ROOT=/media \
    MEDIA_WEB_DIST=/app/apps/web/build \
    MEDIA_PUBLIC_URL=http://127.0.0.1:3000

WORKDIR /app
VOLUME ["/data", "/backups"]
EXPOSE 3000
USER aether:aether
STOPSIGNAL SIGINT

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD curl -f http://127.0.0.1:3000/api/v1/health || exit 1

ENTRYPOINT ["tini", "--"]
CMD ["aether-core"]
