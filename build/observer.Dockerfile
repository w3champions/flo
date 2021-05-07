FROM debian:stable

WORKDIR /flo

RUN apt-get update && \
  apt-get install \
  ca-certificates \
  libssl-dev \
  -qqy \
  --no-install-recommends \
  && rm -rf /var/lib/apt/lists/*

ARG IMAGE_BUILD_DATE=2016-01-01
ENV IMAGE_BUILD_DATE $IMAGE_BUILD_DATE

ENV RUST_BACKTRACE 1

EXPOSE 3556/tcp

COPY release/flo-observer-service flo-observer-service

CMD ["/flo/flo-observer-service"]
