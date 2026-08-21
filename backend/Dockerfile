# syntax=docker/dockerfile:1.7

FROM rust:1.97.1-bookworm AS builder
WORKDIR /build
COPY rust-toolchain.toml ./
COPY backend ./backend
RUN cargo build --locked --release --manifest-path backend/Cargo.toml --bins

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install --no-install-recommends -y ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 10001 arc-admin \
    && useradd --uid 10001 --gid 10001 --no-create-home --shell /usr/sbin/nologin arc-admin
WORKDIR /app
COPY --from=builder /build/backend/target/release/arc-admin-backend /app/arc-admin-backend
COPY --from=builder /build/backend/target/release/migrate /app/migrate
COPY --from=builder /build/backend/target/release/bootstrap_admin /app/bootstrap_admin
COPY --from=builder /build/backend/target/release/archive_audit_logs /app/archive_audit_logs
USER 10001:10001
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=3s --start-period=10s --retries=3 \
    CMD ["curl", "--fail", "--silent", "http://127.0.0.1:8080/api/v1/healthz"]
CMD ["/app/arc-admin-backend"]
