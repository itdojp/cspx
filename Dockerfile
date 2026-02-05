FROM rust:stable AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY crates/cspx-cli/Cargo.toml crates/cspx-cli/Cargo.toml
COPY crates/cspx-core/Cargo.toml crates/cspx-core/Cargo.toml
RUN cargo fetch
COPY . .
RUN cargo build --release --locked

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
RUN useradd -m -u 10001 app
COPY --from=builder /app/target/release/cspx /usr/local/bin/cspx
USER app
ENTRYPOINT ["cspx"]
