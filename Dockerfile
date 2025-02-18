FROM rust:latest as builder

WORKDIR /usr/src/otr-processor
COPY . .

RUN cargo install --path .

CMD ["otr-processor"]

