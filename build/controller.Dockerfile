FROM debian:stable

WORKDIR /flo

ARG IMAGE_BUILD_DATE=2016-01-01
ENV IMAGE_BUILD_DATE $IMAGE_BUILD_DATE

ENV RUST_BACKTRACE 1

EXPOSE 3549/tcp
EXPOSE 3550/tcp

COPY release/flo-controller-service flo-controller-service

CMD ["/flo/flo-controller-service"]