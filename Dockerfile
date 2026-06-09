# Runtime image for the MarretaLang interpreter.
#
# This image is built from the already-compiled release binary, not from source:
#   cargo build --release
#   docker build -t marreta-lang:dev .
#
# Building the binary and this image is the responsibility of this repository.
# Downstream harnesses (examples, benchmarks) consume the published image and
# must never build the runtime themselves.
FROM ubuntu:24.04

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

COPY target/release/marreta /usr/local/bin/marreta

WORKDIR /app
ENTRYPOINT ["marreta"]
CMD ["serve"]
