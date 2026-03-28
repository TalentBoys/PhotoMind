# Stage 1: Build Rust backend
FROM rust:1.94-bookworm AS rust-builder

WORKDIR /app
COPY Cargo.toml Cargo.lock* ./
COPY crates/ crates/

# Build release binary
RUN cargo build --release --bin photomind

# Stage 2: Build React frontend
FROM node:22-bookworm-slim AS web-builder

WORKDIR /app/web
COPY web/package.json web/package-lock.json* ./
RUN npm ci
COPY web/ .
RUN npm run build

# Stage 3: Final runtime image
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy binary
COPY --from=rust-builder /app/target/release/photomind /app/photomind

# Copy frontend build
COPY --from=web-builder /app/web/dist /app/web/dist

# Create data directory
RUN mkdir -p /data

ENV PHOTOMIND_DATA_DIR=/data
ENV PHOTOMIND_ADDR=0.0.0.0:8080
ENV RUST_LOG=info

EXPOSE 8080

VOLUME ["/data", "/photos"]

CMD ["/app/photomind"]
