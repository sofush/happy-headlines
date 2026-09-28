FROM rust:1.98-slim-bookworm AS build

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN cargo build --release

FROM debian:bookworm-slim

COPY --from=build /app/target/release/article-service /usr/local/bin/article-service

CMD ["article-service"]
