# Getting Started

This file covers how to get started with `vertd`.

- [Installing dependencies](#installing-dependencies)
  - [Windows](#windows)
  - [macOS](#macos)
  - [Linux](#linux)
    - [Debian](#debian)
    - [Arch Linux](#arch-linux)
    - [Fedora](#fedora)
- [Downloading the server binaries](#downloading-the-server-binaries)
- [Running `vertd` on Windows](#running-vertd-on-windows)
- [Running `vertd` on macOS/Linux](#running-vertd-on-macoslinux)
  - [Using systemd](#using-systemd)
  - [Using Docker](#using-docker)
- [Manual GPU selection](#manual-gpu-selection)
  - [CLI arguments](#cli-arguments)
  - [Environment variable](#environment-variable)

## Installing dependencies

For `vertd` to work, you'll need to have [FFmpeg](https://ffmpeg.org/) in the directory it is in or in your system PATH.

The instructions below will use a package manager to install it, but you can also download FFmpeg binaries from [their website](https://ffmpeg.org/download.html#build-windows)
instead. You can either put them in your system PATH or in the directory `vertd` is in.

> [!NOTE]  
> Other utilities in the FFmpeg suite like `ffprobe` should also be installed for `vertd` to work properly.

### Windows

Assuming you have [Chocolatey](https://chocolatey.org/install) installed on your system, open a Command Prompt or PowerShell window as an administrator and run:

```shell
$ choco install ffmpeg
```

### macOS

Assuming you have [Homebrew](https://brew.sh/) installed, run:

```shell
$ brew install ffmpeg
```

### Linux

The installation steps depend on your distribution. We will only cover the most commonly used ones.

#### Debian

This should also work for other Debian-based distributions such as Ubuntu, Pop_OS! and Linux Mint.

```shell
$ sudo apt update && sudo apt install -y ffmpeg
```

#### Arch Linux

This should also work for other Arch-based distributions such as Manjaro and EndeavourOS.

```shell
$ sudo pacman -Sy ffmpeg
```

#### Fedora

Use the following command to install FFmpeg on Fedora:

```shell
$ sudo dnf install -y ffmpeg
```

## Downloading the server binaries

Grab the latest `vertd` release for your operating system and architecture from [this page](https://github.com/VERT-sh/vertd/releases).

> [!NOTE]
> If you're using an Intel-based Mac, download the `vertd-mac-x86_64` executable. For Mac computers with Apple silicon (M1 or newer), download `vertd-mac-arm64` instead.

## Running `vertd` on Windows

Simply navigate to the directory where you downloaded the server binary, then open it like any other program.

> [!IMPORTANT]
> It's very likely you will get a SmartScreen pop-up on Windows. You can ignore it by pressing `More info` and then `Run anyway`. However, if you don't trust the file, you can always inspect and compile the code yourself.

## Running `vertd` on macOS/Linux

Assuming you downloaded the `vertd` executable to your Downloads folder, open the Terminal and run the following command to navigate there:

```shell
$ cd ~/Downloads/
```

Then, modify the permissions of the executable and run it by using:

```shell
$ chmod +x <vertd filename>
$ ./<vertd filename>
```

Replace `<vertd filename>` with the name of the file you just downloaded (e.g. `vertd-mac-arm64`)

> [!TIP]
> For Arch Linux users, there's a **community-made** [`vertd-git`](https://aur.archlinux.org/packages/vertd-git) AUR package you can use.

### Using systemd

Assuming your `vertd` executable is called `vertd-linux-x86_64` and is on the `~/Downloads` folder, run:

```shell
$ sudo mv ~/Downloads/vertd-linux-x86_64 /usr/bin/vertd
```

Create a service file (thanks [@mqus](https://github.com/mqus) and [@claymorwan](https://github.com/claymorwan)!):

```shell
$ sudo tee /etc/systemd/system/vertd.service<<EOF
[Unit]
Description=vertd - media conversion services
Requires=network.target
After=network.target

[Service]
User=vertd
Group=vertd
DynamicUser=true
Restart=on-failure
EnvironmentFile=-/etc/conf.d/vertd
ExecStart=/usr/bin/vertd
NoNewPrivileges=true
ProtectHome=true
ProtectSystem=strict

CacheDirectory=vertd
CacheDirectoryMode=0700
WorkingDirectory=/var/cache/vertd
ReadWritePaths=/var/cache/vertd
NoExecPaths=/var/cache/vertd

[Install]
WantedBy=multi-user.target
EOF
```

Reload the system daemon:

```shell
$ sudo systemctl daemon-reload
```

And finally, enable (and start) the `vertd` service:

```shell
$ sudo systemctl enable --now vertd
```

To check the status of `vertd`, run:

```shell
$ sudo systemctl status vertd
```

You can also try opening http://localhost:24153/api/version in your favorite web browser.

### Using Docker

Check out the [Docker Setup](./DOCKER_SETUP.md) page.

## Manual GPU selection

If `vertd` can't detect your GPU type or detects the wrong one, you can force `vertd` to use hardware acceleration for a specific vendor manually: `nvidia`, `amd`, `intel`, `apple`, or `cpu` (for software encoding).

### CLI arguments

Pass in the `--gpu` (or `-gpu`) argument with your vendor type (`nvidia`/`amd`/`intel`/`apple`/`cpu`) while starting `vertd`. For example:

```shell
$ ./vertd --gpu intel
```

To use CPU rendering (software encoding):

```shell
$ ./vertd --gpu cpu
```

### Environment variable

You can set the `VERTD_FORCE_GPU` environment variable with your vendor type (`nvidia`/`amd`/`intel`/`apple`/`cpu`) in your shell config, or in your shell session temporarily. For example:

```shell
$ VERTD_FORCE_GPU=intel ./vertd
```

Or to force CPU rendering:

```shell
$ VERTD_FORCE_GPU=cpu ./vertd
```

### Automatic CPU fallback

If GPU detection fails for any reason, `vertd` will automatically fall back to CPU rendering. You'll see warning messages in the logs:

```text
[ERROR] failed to get GPU vendor: <error details>
[WARN] falling back to CPU rendering (software encoding) -- this will be slower than GPU acceleration
```

This ensures that `vertd` continues to work even on systems without GPU support, albeit slower than with GPU acceleration.

## VA-API device path configuration

By default, `vertd` uses `/dev/dri/renderD128` as the VA-API device path for Intel and AMD GPUs on Linux. If your system uses a different device path (e.g., `/dev/dri/renderD129`), you can configure it:

### CLI arguments

Use the `--vaapi-device` (or `-vaapi-device`) argument when starting `vertd`:

```shell
$ ./vertd --vaapi-device /dev/dri/renderD129
```

### Environment variable

Set the `VERTD_VAAPI_DEVICE_PATH` environment variable:

```shell
$ VERTD_VAAPI_DEVICE_PATH=/dev/dri/renderD129 ./vertd
```

> [!NOTE]
> This setting only affects Intel and AMD GPUs on Linux. It has no effect on NVIDIA GPUs, Apple GPUs, or other platforms.
