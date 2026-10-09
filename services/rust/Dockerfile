# Builds ONE Rust service of the workspace (core-api | rule-service | graph-service).
#
#   docker build -f services/rust/Dockerfile --build-arg SERVICE=rule-service -t fraud-platform/rule-service .
#
# Build context = repository root (the image embeds db/migrations for `core-api migrate`).
# Stages: chef (tooling) → planner (dependency recipe) → builder (cached deps, then app) → runtime (distroless).
# No BuildKit-only syntax (RUN --mount) so it also builds with the classic builder; dependency caching comes
# from the cargo-chef "cook" layer, which is only invalidated when Cargo.toml/Cargo.lock change.

ARG RUST_VERSION=1.96.1
ARG CARGO_CHEF_VERSION=0.1.71

FROM rust:${RUST_VERSION}-slim-bookworm AS chef
ARG CARGO_CHEF_VERSION
RUN cargo install cargo-chef --locked --version ${CARGO_CHEF_VERSION}
WORKDIR /build

# ---- planner: compute the dependency recipe (changes only when Cargo.toml/lock change)
FROM chef AS planner
COPY services/rust/ .
# The image's pinned toolchain wins over rust-toolchain.toml ("stable" would trigger a download).
RUN rm -f rust-toolchain.toml && cargo chef prepare --recipe-path recipe.json

# ---- builder: build dependencies (cached layer), then the service
FROM chef AS builder
ARG SERVICE
RUN test -n "${SERVICE}" || (echo "build-arg SERVICE is required" && exit 1)
COPY --from=planner /build/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json --package ${SERVICE}
COPY services/rust/ .
RUN rm -f rust-toolchain.toml
RUN cargo build --release --locked --package ${SERVICE} \
 && cp target/release/${SERVICE} /build/service

# ---- runtime: no shell, no package manager, non-root (uid 65532)
FROM gcr.io/distroless/cc-debian12:nonroot AS runtime
ARG SERVICE
LABEL org.opencontainers.image.title="fraud-platform/${SERVICE}" \
      org.opencontainers.image.authors="Hendri Karisma <situkangsayur@gmail.com>" \
      org.opencontainers.image.source="https://github.com/situkangsayur/fraud_detection_engine"
WORKDIR /app
COPY --from=builder --chown=nonroot:nonroot /build/service /app/service
COPY --chown=nonroot:nonroot db/migrations /app/migrations
ENV MIGRATIONS_DIR=/app/migrations \
    RUST_LOG=info
USER nonroot:nonroot
EXPOSE 8080 8081 8082
ENTRYPOINT ["/app/service"]
CMD ["serve"]
