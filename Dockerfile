FROM rust:1.86 AS builder

WORKDIR /build

COPY . .

RUN cargo build --release

FROM ubuntu:24.04

WORKDIR /app

ARG DEBIAN_FRONTEND="noninteractive"

ENV XDG_RUNTIME_DIR="/tmp"
ENV NVIDIA_VISIBLE_DEVICES="all"
ENV NVIDIA_DRIVER_CAPABILITIES="all"

# nvidia-container-toolkit injects the driver libs (nvenc, cuda, vulkan) via
# the env vars above, so a cuda base image would only add dead weight.
# https://github.com/NVIDIA/nvidia-container-toolkit/issues/140#issuecomment-1927273909
RUN apt-get update && \
    apt-get install -y --no-install-recommends \
    curl \
    ca-certificates \
    ffmpeg \
    mesa-va-drivers \
    libvulkan1 \
    libglvnd0 \
    libgl1 \
    libglx0 \
    libegl1 \
    libgles2 && \
    if [ "$(dpkg --print-architecture)" = "amd64" ]; then \
        apt-get install -y --no-install-recommends intel-media-va-driver; \
    fi && \
    apt-get clean && \
    rm -rf /var/lib/apt/lists/* /tmp/* /var/tmp/*

COPY --from=builder /build/target/release/vertd ./vertd

EXPOSE 24153/tcp

HEALTHCHECK --interval=30s --timeout=10s --start-period=5s --retries=3 \
    CMD sh -c "curl --fail --silent --output /dev/null http://localhost:${PORT:-24153}/api/version || exit 1"

ENTRYPOINT ["./vertd"]
