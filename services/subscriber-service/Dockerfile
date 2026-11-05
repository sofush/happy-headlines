FROM rust:1.98-slim-bookworm AS build

WORKDIR /app

COPY --from=feature-lib . /feature-lib

COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN cargo build --release

FROM debian:bookworm-slim

# reqwest's rustls backend loads the system root store at client-build time,
# so the runtime image needs ca-certificates even for plain HTTP calls.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=build /app/target/release/subscriber-service /usr/local/bin/subscriber-service

CMD ["subscriber-service"]
