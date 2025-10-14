# Docker Setup

This file covers how to set up `vertd` using Docker.

- [For NVIDIA users](#for-nvidia-users)
- [Building an image](#building-an-image)
- [Manually](#manually)
  - [Intel and AMD GPUs](#intel-and-amd-gpus)
  - [NVIDIA GPUs](#nvidia-gpus)
- [With Compose (recommended)](#with-compose-recommended)
  - [Intel and AMD GPUs](#intel-and-amd-gpus-1)
  - [NVIDIA GPUs](#nvidia-gpus-1)
- [Manual GPU selection](#manual-gpu-selection)

> [!CAUTION]
> Docker Desktop on macOS and Windows is unsupported.
> It might work if you have a NVIDIA GPU, but no guarantees. You're on your own.

## For NVIDIA users

You'll need to install the [NVIDIA Container Toolkit](https://docs.nvidia.com/datacenter/cloud-native/container-toolkit/latest/install-guide.html) and configure Docker to use the NVIDIA Container Runtime by running:

```shell
$ sudo nvidia-ctk runtime configure --runtime=docker
$ sudo systemctl restart docker
```

> [!NOTE]  
> The commands above assume you're **not** running Docker in rootless mode. If you are, check the NVIDIA documentation for more details.

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

```diff
$ docker run -d \
    --name vertd \
    --restart=unless-stopped \
+   --device=/dev/dri:/dev/dri \
    -p 24153:24153 \
    ghcr.io/vert-sh/vertd:latest
```

### NVIDIA GPUs

If you have a NVIDIA GPU, you'll need to add the `--runtime=nvidia` and `--gpus all` parameters to your `docker run` command. It should end up looking something like this:

```diff
$ docker run -d \
    --name vertd \
    --restart=unless-stopped \
+   --runtime=nvidia \
+   --gpus all \
    -p 24153:24153 \
    ghcr.io/vert-sh/vertd:latest
```

## With Compose (recommended)

There's a [`docker-compose.yml`](../docker-compose.yml) file in this repository which you can use to easily get started.

### Intel and AMD GPUs

If you're using an Intel or AMD GPU, add the following to the `vertd` service in your Docker Compose file:

```yaml
devices:
  - /dev/dri
```

Assuming you're using the [`docker-compose.yml`](../docker-compose.yml) file from this repository, you should also remove the following NVIDIA specific settings from it:

```diff
- runtime: nvidia
- deploy:
-   resources:
-     reservations:
-       devices:
-         - driver: nvidia
-           count: all
-           capabilities: [gpu]
```

<details>
<summary>Full docker-compose.yml example for Intel/AMD GPUs</summary>

```yml
services:
  vertd:
    image: ghcr.io/vert-sh/vertd:latest
    container_name: vertd
    restart: unless-stopped
    ports:
      - "24153:24153"
    devices:
      - /dev/dri
```

</details>

Finally, run the following command to bring the stack up:

```bash
docker compose up
```

If you see a `detected an Intel GPU` or `detected an AMD GPU` message, you should be ready to go.

### NVIDIA GPUs

If you're using the [`docker-compose.yml`](../docker-compose.yml) file we provide in this repository, you shouldn't need to do any changes. Otherwise, add the following settings to the `vertd` service:

```diff
+ runtime: nvidia
+ deploy:
+   resources:
+     reservations:
+       devices:
+         - driver: nvidia
+           count: all
+           capabilities: [gpu]
```

Finally, bring the stack up by using:

```bash
docker compose up
```

<details>
<summary>Full docker-compose.yml example for NVIDIA GPUs</summary>

```yml
services:
  vertd:
    image: ghcr.io/vert-sh/vertd:latest
    container_name: vertd
    restart: unless-stopped
    ports:
      - "24153:24153"
    runtime: nvidia
    deploy:
      resources:
        reservations:
          devices:
            - driver: nvidia
              count: all
              capabilities: [gpu]
```

</details>

If you see a `detected a NVIDIA GPU` message without any warnings, you should be ready to go.

## Manual GPU selection

If the automatic GPU detection doesn't work correctly, you can manually force `vertd` to use a specific GPU vendor by setting the `VERTD_FORCE_GPU` environment variable.

Valid values for `VERTD_FORCE_GPU` are: `nvidia`, `amd`, `intel`, or `apple`.

For Docker Compose configurations, add:

```yaml
environment:
  - VERTD_FORCE_GPU=nvidia
```

For `docker run` commands, use:

```diff
$ docker run -d \
    --name vertd \
    --restart=unless-stopped \
+   -e VERTD_FORCE_GPU=nvidia \
    -p 24153:24153 \
    ghcr.io/vert-sh/vertd:latest
```
