FROM gcr.io/distroless/static:nonroot@sha256:188ddfb9e497f861177352057cb21913d840ecae6c843d39e00d44fa64daa51c AS runtime

# Used for CI builds that cross-compile outside of the container build.
# Assumes a directory layout of bin/rapla-ical-proxy-{arm64,amd64,...}.
ARG TARGETARCH
COPY rapla-ical-proxy.${TARGETARCH} /usr/local/bin/rapla-ical-proxy

ENV RAPLA_ADDRESS=0.0.0.0:8080
EXPOSE 8080

USER 65532:65532

ENTRYPOINT [ "rapla-ical-proxy" ]
