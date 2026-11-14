FROM rust:1.99-slim-bookworm AS build

WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release --locked

FROM debian:bookworm-slim

RUN useradd -m -u 1001 bridge
COPY --from=build /src/target/release/freepi-bridge /usr/local/bin/freepi-bridge
USER bridge

ENV HOST=0.0.0.0
ENV PORT=7860

EXPOSE 7860

HEALTHCHECK --interval=30s --timeout=5s --start-period=5s --retries=3 \
  CMD ["freepi-bridge", "--health"]

CMD ["freepi-bridge", "start"]
