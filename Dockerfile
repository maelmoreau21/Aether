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

WORKDIR /build

# Copy workspace manifests
COPY Cargo.toml Cargo.lock ./
COPY crates/shared/Cargo.toml ./crates/shared/
COPY crates/node/Cargo.toml ./crates/node/
COPY crates/core/Cargo.toml ./crates/core/

# Scaffold dummy source hierarchy for dependency precompilation caching
# Explicit test targets declared in crates/core/Cargo.toml (p0, catalog) must exist
RUN mkdir -p crates/shared/src crates/node/src crates/core/src crates/core/tests && \
    echo 'pub fn dummy() {}' > crates/shared/src/lib.rs && \
    echo 'pub fn dummy() {}' > crates/node/src/lib.rs && \
    echo 'pub fn dummy() {}' > crates/core/src/lib.rs && \
    echo 'fn main() {}' > crates/core/src/main.rs && \
    echo 'fn main() {}' > crates/core/src/bootstrap.rs && \
    touch crates/core/tests/p0.rs crates/core/tests/catalog.rs

# Precompile workspace dependencies
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    cargo build --release --locked --bin media-poc --bin media-bootstrap

# Copy actual workspace crates (shared contracts, node, and core with migrations)
COPY crates/ ./crates/

# Clean dummy binary/deps artifacts, recompile actual release binaries, and strip symbols
RUN touch crates/shared/src/lib.rs crates/node/src/lib.rs crates/core/src/lib.rs crates/core/src/main.rs crates/core/src/bootstrap.rs && \
    rm -f target/release/deps/*media_contracts* target/release/deps/*media_node* target/release/deps/*media_poc* target/release/deps/*media_bootstrap* target/release/media-poc target/release/media-bootstrap && \
    cargo build --release --locked --bin media-poc --bin media-bootstrap && \
    strip --strip-all target/release/media-poc target/release/media-bootstrap

# ==============================================================================
# Stage 3: Minimal Linux x86_64 Runtime Environment
# ==============================================================================
FROM debian:bookworm-slim AS runtime

# Install system dependencies with zero desktop/recommended package bloat
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates \
        curl \
        ffmpeg \
        tini \
    && rm -rf /var/lib/apt/lists/*

# Create unprivileged non-root user and group aether (UID 10001, GID 10001)
RUN groupadd --gid 10001 aether \
    && useradd --uid 10001 --gid 10001 --create-home --home-dir /app --shell /usr/sbin/nologin aether

# Pre-create directory layout with exact permissions
# /data: Writable persistent volume for SQLite (core.sqlite3, WAL, SHM) - mode 0750
# /media: Mount root for read-only media libraries (:ro) - mode 0755
# /app: Application directory housing static web assets - mode 0755
RUN mkdir -p /data /media /app/apps/web/build \
    && chown -R aether:aether /data /media /app \
    && chmod 0750 /data \
    && chmod 0755 /media /app

# Copy stripped binaries from Rust builder stage to /usr/local/bin
COPY --from=rust-builder --chmod=0755 /build/target/release/media-poc /usr/local/bin/media-poc
COPY --from=rust-builder --chmod=0755 /build/target/release/media-bootstrap /usr/local/bin/media-bootstrap

# Copy static frontend distribution from Web builder stage
COPY --from=web-builder --chown=aether:aether /app/apps/web/build /app/apps/web/build

# Application runtime environment defaults
ENV MEDIA_PORT=3000 \
    MEDIA_BIND_ADDR=0.0.0.0 \
    MEDIA_DB_PATH=/data/core.sqlite3 \
    MEDIA_LIBRARY_ROOT=/media \
    MEDIA_WEB_DIST=/app/apps/web/build \
    MEDIA_PUBLIC_URL=http://127.0.0.1:3000

# Container topology & security directives
WORKDIR /app
VOLUME ["/data"]
EXPOSE 3000
USER aether:aether

# Signal forwarding alignment: Docker stop triggers SIGINT to match tokio::signal::ctrl_c()
STOPSIGNAL SIGINT

# Healthcheck probe using internal loopback endpoint (resilient to external MEDIA_PUBLIC_URL)
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD curl -f http://127.0.0.1:3000/api/v1/health || exit 1

# Process init system & default command
ENTRYPOINT ["tini", "--"]
CMD ["media-poc"]
