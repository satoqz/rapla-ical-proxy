FROM gcr.io/distroless/static:nonroot@sha256:6ec5aa99dc335666e79dc64e4a6c8b89c33a543a1967f20d360922a80dd21f02 AS runtime

# Used for CI builds that cross-compile outside of the container build.
# Assumes a directory layout of bin/rapla-ical-proxy-{arm64,amd64,...}.
ARG TARGETARCH
COPY rapla-ical-proxy.${TARGETARCH} /usr/local/bin/rapla-ical-proxy

ENV RAPLA_ADDRESS=0.0.0.0:8080
EXPOSE 8080

USER 65532:65532

ENTRYPOINT [ "rapla-ical-proxy" ]
