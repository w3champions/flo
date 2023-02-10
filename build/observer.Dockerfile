FROM rust:1-bullseye AS builder

WORKDIR /usr/local/build

COPY . .

RUN rustup component add rustfmt

RUN cargo build -p flo-observer-service --release

FROM debian:bullseye-slim

RUN apt-get update && apt-get install -y ca-certificates libssl-dev libpq-dev && rm -rf /var/lib/apt/lists/*

WORKDIR /usr/local/s2

ARG IMAGE_BUILD_DATE=2016-01-01
ENV IMAGE_BUILD_DATE $IMAGE_BUILD_DATE

ENV RUST_BACKTRACE 1

EXPOSE 3556/tcp

COPY --from=builder /usr/local/build/target/release/flo-observer-service /usr/local/flo/flo-observer-service

CMD ["/usr/local/flo/flo-observer-service"]