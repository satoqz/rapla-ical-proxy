# used for CI builds that cross-compile outside of the container build.
# assumes a directory layout of bin/rapla-ical-proxy-{arm64,amd64,...}

FROM gcr.io/distroless/static:nonroot@sha256:d71f4b239be2d412017b798a0a401c44c3049a3ca454838473a4c32ed076bfea

ARG TARGETARCH
COPY rapla-ical-proxy-${TARGETARCH} /usr/local/bin/rapla-ical-proxy

USER 65532:65532
EXPOSE 8080

ENTRYPOINT [ "rapla-ical-proxy" ]
CMD [ "--address=0.0.0.0:8080", "--cache" ]
