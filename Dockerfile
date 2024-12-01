FROM docker.io/rust:alpine3.20@sha256:2f42ce0d00c0b14f7fd84453cdc93ff5efec5da7ce03ead6e0b41adb1fbe834e AS chef
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

FROM gcr.io/distroless/static:nonroot@sha256:d71f4b239be2d412017b798a0a401c44c3049a3ca454838473a4c32ed076bfea 
USER 65532:65532
EXPOSE 8080
COPY --from=builder /build/target/release/rapla-ical-proxy /usr/local/bin/rapla-ical-proxy
ENTRYPOINT [ "rapla-ical-proxy" ]
CMD [ "--address=0.0.0.0:8080", "--cache" ]
