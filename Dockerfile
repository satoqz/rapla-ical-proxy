FROM rust:alpine AS builder

RUN apk add --no-cache musl-dev

ADD Cargo.toml Cargo.lock ./
ADD src ./src

RUN cargo fetch --locked
RUN cargo install --locked --path .


FROM scratch AS runner

COPY --from=builder /usr/local/cargo/bin/rapla-proxy /bin/rapla-proxy
ENV PATH /bin

ENV IP 0.0.0.0
EXPOSE 8080

CMD ["rapla-proxy"]