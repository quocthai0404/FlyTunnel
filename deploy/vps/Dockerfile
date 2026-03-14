# syntax=docker/dockerfile:1.7

ARG FRP_VERSION=0.67.0

FROM --platform=$BUILDPLATFORM debian:bookworm-slim AS downloader

ARG FRP_VERSION
ARG TARGETARCH

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl tar \
    && rm -rf /var/lib/apt/lists/*

RUN case "$TARGETARCH" in \
        amd64) FRP_ARCH="amd64" ;; \
        arm64) FRP_ARCH="arm64" ;; \
        *) echo "Unsupported TARGETARCH: $TARGETARCH" >&2; exit 1 ;; \
    esac \
    && curl -fsSL -o /tmp/frp.tar.gz "https://github.com/fatedier/frp/releases/download/v${FRP_VERSION}/frp_${FRP_VERSION}_linux_${FRP_ARCH}.tar.gz" \
    && mkdir -p /tmp/frp-extract /opt/frp \
    && tar -xzf /tmp/frp.tar.gz -C /tmp/frp-extract \
    && cp "/tmp/frp-extract/frp_${FRP_VERSION}_linux_${FRP_ARCH}/frps" /opt/frp/frps \
    && chmod +x /opt/frp/frps

FROM debian:bookworm-slim

ARG FRP_VERSION

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl gettext-base \
    && rm -rf /var/lib/apt/lists/*

RUN groupadd --system frp \
    && useradd --system --gid frp --create-home --home-dir /home/frp frp \
    && mkdir -p /etc/frp /opt/flytunnel \
    && chown -R frp:frp /etc/frp /opt/flytunnel /home/frp

COPY --from=downloader /opt/frp/frps /usr/local/bin/frps
COPY frps.toml.template /opt/flytunnel/frps.toml.template

ENV FRP_VERSION=${FRP_VERSION}

USER frp:frp
WORKDIR /home/frp

EXPOSE 7000 25565 7500

ENTRYPOINT ["/bin/sh", "-ec", "for var in FRPS_BIND_PORT FRPS_TOKEN FRPS_DASHBOARD_PORT FRPS_DASHBOARD_USER FRPS_DASHBOARD_PASSWORD; do eval \"value=\\${$var:-}\"; if [ -z \"$value\" ]; then echo \"Missing required environment variable: $var\" >&2; exit 1; fi; done; envsubst '${FRPS_BIND_PORT} ${FRPS_TOKEN} ${FRPS_DASHBOARD_PORT} ${FRPS_DASHBOARD_USER} ${FRPS_DASHBOARD_PASSWORD}' < /opt/flytunnel/frps.toml.template > /etc/frp/frps.toml; exec /usr/local/bin/frps -c /etc/frp/frps.toml"]
