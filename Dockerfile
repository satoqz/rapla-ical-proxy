FROM docker.io/rust:alpine3.20@sha256:838d384a1138fe1f2e448e3901bb3d23683570ba3dca581160880ffad760332b AS chef
WORKDIR /build
RUN apk add --no-cache musl-dev git
RUN cargo install cargo-chef

FROM chef AS planner
COPY . .
RUN cargo chef prepare

FROM chef AS builder
COPY --from=planner /build/recipe.json .
RUN cargo chef cook --locked --release
COPY . .
RUN cargo build --frozen --release

FROM gcr.io/distroless/static:nonroot@sha256:6cd937e9155bdfd805d1b94e037f9d6a899603306030936a3b11680af0c2ed58 AS runtime
USER 65532:65532
EXPOSE 8080
ENTRYPOINT [ "rapla-ical-proxy" ]
CMD [ "--address=0.0.0.0:8080", "--cache" ]

FROM runtime AS external-build
COPY rapla-ical-proxy /usr/local/bin/rapla-ical-proxy

FROM runtime AS docker-build
COPY --from=builder /build/target/release/rapla-ical-proxy /usr/local/bin/rapla-ical-proxy
