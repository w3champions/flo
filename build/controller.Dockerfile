FROM debian:stable

RUN apt-get update && \
  apt-get install \
  libpq-dev \
  -qqy \
  --no-install-recommends \
  && rm -rf /var/lib/apt/lists/*

WORKDIR /flo

ARG IMAGE_BUILD_DATE=2016-01-01
ENV IMAGE_BUILD_DATE $IMAGE_BUILD_DATE

ENV RUST_BACKTRACE 1

EXPOSE 3549/tcp
EXPOSE 3550/tcp

COPY release/flo-controller-service flo-controller-service

CMD ["/flo/flo-controller-service"]