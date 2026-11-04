FROM rust:1.98-slim-bookworm AS build

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN cargo build --release

FROM debian:bookworm-slim

# reqwest's rustls backend loads the system root store at client-build time,
# so the runtime image needs ca-certificates even for plain HTTP calls.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=build /app/target/release/newsletter-service /usr/local/bin/newsletter-service

CMD ["newsletter-service"]
