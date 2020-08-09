FROM debian:stable

RUN apt-get update && \
  apt-get install \
  ca-certificates \
  curl \
  unzip \
  pkgconf \
  build-essential \
  cmake \
  libbz2-dev \
  -qqy \
  --no-install-recommends \
  && rm -rf /var/lib/apt/lists/*

RUN curl https://sh.rustup.rs -sSf | sh -s -- --default-toolchain stable -y
ENV PATH "$HOME/.cargo/bin:$PATH"