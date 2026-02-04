FROM stagex/pallet-rust@sha256:84621c4c29330c8a969489671d46227a0a43f52cdae243f5b91e95781dbfe5ed AS build
WORKDIR /build
COPY ./src ./src
COPY ./Cargo.toml ./Cargo.lock .
ENV RUSTFLAGS="-C target-feature=+crt-static"
RUN --mount=type=cache,target=/root/.cargo \
    cargo fetch --locked
RUN --network=none \
    --mount=type=cache,target=/root/.cargo \
    --mount=type=cache,target=/build/target \
<<-EOF
    set -eux
    ARCH=$(uname -m)
    cargo build --target "${ARCH}-unknown-linux-musl" --frozen --release
    mkdir -p /rootfs/usr/bin
    cp "target/${ARCH}-unknown-linux-musl/release/rapla-ical-proxy" /rootfs/usr/bin
EOF

FROM scratch
COPY --from=build /rootfs /
EXPOSE 8080
ENV RAPLA_ADDRESS=0.0.0.0:8080
ENTRYPOINT ["rapla-ical-proxy"]
