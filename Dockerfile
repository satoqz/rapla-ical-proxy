# Used for CI builds that cross-compile outside of the container build.
# Assumes a directory layout of bin/rapla-ical-proxy-{arm64,amd64,...}.
ARG TARGETARCH

FROM gcr.io/distroless/static:nonroot@sha256:6cd937e9155bdfd805d1b94e037f9d6a899603306030936a3b11680af0c2ed58 AS runtime
COPY rapla-ical-proxy.${TARGETARCH} /usr/local/bin/rapla-ical-proxy
ENV RAPLA_ADDRESS 0.0.0.0:8080
EXPOSE 8080
USER 65532:65532
ENTRYPOINT [ "rapla-ical-proxy" ]
