FROM rust:1.86 AS builder

WORKDIR /build

COPY . .

RUN cargo build --release

FROM nvidia/cuda:12.8.0-base-ubuntu24.04

WORKDIR /app

ARG DEBIAN_FRONTEND="noninteractive"

ENV XDG_RUNTIME_DIR="/tmp"
ENV NVIDIA_VISIBLE_DEVICES="all"
ENV NVIDIA_DRIVER_CAPABILITIES="all"

COPY --from=builder /build/target/release/vertd ./vertd

# https://github.com/NVIDIA/nvidia-container-toolkit/issues/140#issuecomment-1927273909
RUN apt-get update && \
    apt-get install -y \
    curl \
    ffmpeg \
    mesa-va-drivers \
    libglvnd0 \
    libgl1 \
    libglx0 \
    libegl1  \
    libgles2  \
    libxcb1-dev \
    vulkan-tools \
    mesa-utils && \
    if [ "$(dpkg --print-architecture)" = "amd64" ]; then \
        apt-get install -y intel-media-va-driver; \
    fi

RUN rm -rf \
    /tmp/* \
    /var/lib/apt/lists/* \
    /var/tmp/*

EXPOSE 24153/tcp

HEALTHCHECK --interval=30s --timeout=10s --start-period=5s --retries=3 \
    CMD sh -c "curl --fail --silent --output /dev/null http://localhost:${PORT:-24153}/api/version || exit 1"

ENTRYPOINT ["./vertd"]
