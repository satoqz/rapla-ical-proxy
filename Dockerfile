FROM gcr.io/distroless/static:nonroot@sha256:963fa6c544fe5ce420f1f54fb88b6fb01479f054c8056d0f74cc2c6000df5240 AS runtime

# Used for CI builds that cross-compile outside of the container build.
# Assumes a directory layout of bin/rapla-ical-proxy-{arm64,amd64,...}.
ARG TARGETARCH
COPY rapla-ical-proxy.${TARGETARCH} /usr/local/bin/rapla-ical-proxy

ENV RAPLA_ADDRESS=0.0.0.0:8080
EXPOSE 8080

USER 65532:65532

ENTRYPOINT [ "rapla-ical-proxy" ]
