# Docker

This file covers how to set up `vertd` using Docker.

- [For NVIDIA users](#for-nvidia-users)
- [Building an image](#building-an-image)
- [Manually](#manually)
  - [Intel and AMD GPUs](#intel-and-amd-gpus)
  - [NVIDIA GPUs](#nvidia-gpus)
- [With Compose (recommended)](#with-compose)
  - [Intel and AMD GPUs](#intel-and-amd-gpus-1)
  - [NVIDIA GPUs](#nvidia-gpus-1)

> [!CAUTION]
> Docker Desktop on macOS and Windows is currently unsupported.
> It might work if you have a NVIDIA GPU, but no guarantees. You're on your own.

## For NVIDIA users

You'll need to install the [NVIDIA Container Toolkit](https://docs.nvidia.com/datacenter/cloud-native/container-toolkit/latest/install-guide.html) and configure Docker to use the NVIDIA Container Runtime by running:

```shell
$ sudo nvidia-ctk runtime configure --runtime=docker
$ sudo systemctl restart docker
```

> [!NOTE]  
> The commands above assume you're **not** running Docker in rootless mode. If you are, check the NVIDIA documentation for more details.

---

## Building an image

Clone the repository:

```shell
$ git clone https://github.com/VERT-sh/vertd
$ cd vertd/
```

Then, run the following command to build a Docker image for `vertd` with the `ghcr.io/vert-sh/vertd:latest` tag:

```shell
$ docker build -t ghcr.io/vert-sh/vertd:latest .
```

---

## Manually

In any case, your `docker run` command should at least have the following parameters:

```shell
$ docker run -d \
    --name vertd \
    --restart=unless-stopped \
    -p 24153:24153 \
    ghcr.io/vert-sh/vertd:latest
```

### Intel and AMD GPUs

If you have an Intel or AMD GPU, you'll need to add the `--device=/dev/dri:/dev/dri` parameter to your `docker run` command. It should end up looking something like this:

```shell
$ docker run -d \
    --name vertd \
    --restart=unless-stopped \
    --device=/dev/dri:/dev/dri \
    -p 24153:24153 \
    ghcr.io/vert-sh/vertd:latest
```

### NVIDIA GPUs

If you have a NVIDIA GPU, you'll need to add the `--runtime=nvidia` and `--gpus all` parameters to your `docker run` command. It should end up looking something like this:

```shell
$ docker run -d \
    --name vertd \
    --restart=unless-stopped \
    --runtime=nvidia \
    --gpus all \
    -p 24153:24153 \
    ghcr.io/vert-sh/vertd:latest
```

---

## With Compose (recommended)

There's a [`docker-compose.yml`](../docker-compose.yml) file in this repository which you can use to easily get started.

### Intel and AMD GPUs

If you're using an Intel or AMD GPU, add the following to the `vertd` service in your Docker Compose file:

```yaml
devices:
  - /dev/dri
```

Assuming you're using the [`docker-compose.yml`](../docker-compose.yml) file from this repository, you should also remove the following NVIDIA specific settings from it:

```yaml
runtime: nvidia
deploy:
  resources:
    reservations:
      devices:
        - driver: nvidia
          count: all
          capabilities: [gpu]
```

Finally, run the following command to bring the stack up:

```shell
$ docker compose up
```

If you see a `detected an Intel GPU` or `detected an AMD GPU` message, you should be ready to go.

### NVIDIA GPUs

If you're using the [`docker-compose.yml`](../docker-compose.yml) file we provide in this repository, you shouldn't need to do any changes. Otherwise, add the following settings to the `vertd` service:

```yaml
runtime: nvidia
deploy:
  resources:
  reservations:
    devices:
      - driver: nvidia
        count: all
        capabilities: [gpu]
```

Finally, bring the stack up by using:

```shell
$ docker compose up
```

If you see a `detected a NVIDIA GPU` message without any warnings, you should be ready to go.
