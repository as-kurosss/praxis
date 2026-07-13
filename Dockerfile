# ─── Stage 1: Build ──────────────────────────────────────────────────────────
FROM rust:latest AS build

WORKDIR /build

# Copy manifests first for dependency caching
COPY Cargo.toml Cargo.lock ./
COPY crates/core/Cargo.toml crates/core/
COPY crates/runtime/Cargo.toml crates/runtime/
COPY crates/cli/Cargo.toml crates/cli/
COPY crates/api-server/Cargo.toml crates/api-server/
COPY crates/mcp/Cargo.toml crates/mcp/
COPY examples/Cargo.toml examples/

# Create dummy sources so `cargo build` can cache dependencies
RUN mkdir -p crates/core/src crates/runtime/src crates/cli/src \
             crates/api-server/src crates/mcp/src examples/src \
    && echo "fn main() {}" > crates/cli/src/main.rs \
    && echo "fn main() {}" > crates/api-server/src/main.rs \
    && for dir in core runtime mcp; do \
         echo "// dummy" > "crates/$dir/src/lib.rs"; \
       done \
    && echo "fn main() {}" > examples/src/main.rs

# Build dependencies (this layer is cached as long as Cargo.lock doesn't change)
RUN cargo build --release --workspace 2>/dev/null || true

# Now copy real sources
COPY . .

# Touch sources to force rebuild (the dummy build above ensures deps are cached)
RUN find . -name "*.rs" -exec touch {} \;

# Full release build
RUN cargo build --release --workspace

# ─── Stage 2: Runtime ───────────────────────────────────────────────────────
FROM debian:bookworm-slim

# Install runtime dependencies (CA certificates for HTTPS)
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Copy binaries from the build stage
COPY --from=build /build/target/release/praxis-api-server /usr/local/bin/praxis-api-server
COPY --from=build /build/target/release/praxis /usr/local/bin/praxis

# Create a non-root user
RUN useradd -m -u 1001 praxis

# Data directory
RUN mkdir -p /data && chown praxis:praxis /data

USER praxis

ENV PRAXIS_DATA=/data
ENV PRAXIS_HOST=0.0.0.0
ENV PRAXIS_PORT=3000

EXPOSE 3000

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD ["praxis-api-server", "--help"]

ENTRYPOINT ["praxis-api-server"]
