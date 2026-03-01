FROM gcr.io/distroless/static:nonroot@sha256:f512d819b8f109f2375e8b51d8cfd8aafe81034bc3e319740128b7d7f70d5036 AS runtime

# Used for CI builds that cross-compile outside of the container build.
# Assumes a directory layout of bin/rapla-ical-proxy-{arm64,amd64,...}.
ARG TARGETARCH
COPY rapla-ical-proxy.${TARGETARCH} /usr/local/bin/rapla-ical-proxy

ENV RAPLA_ADDRESS=0.0.0.0:8080
EXPOSE 8080

USER 65532:65532

ENTRYPOINT [ "rapla-ical-proxy" ]
