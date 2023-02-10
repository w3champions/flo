FROM rust:1-bullseye AS builder

WORKDIR /usr/local/build

COPY . .

RUN rustup component add rustfmt

RUN cargo build -p flo-controller-service --release

FROM debian:bullseye-slim

RUN apt-get update && apt-get install -y ca-certificates libssl-dev libpq-dev && rm -rf /var/lib/apt/lists/*

WORKDIR /usr/local/s2

ARG IMAGE_BUILD_DATE=2016-01-01
ENV IMAGE_BUILD_DATE $IMAGE_BUILD_DATE

ENV RUST_BACKTRACE 1

EXPOSE 3552/udp
EXPOSE 3553/tcp
EXPOSE 3554/tcp
EXPOSE 3555/tcp

COPY --from=builder /usr/local/build/target/release/flo-node-service /usr/local/flo/flo-node-service

CMD ["/usr/local/flo/flo-node-service"]