# Getting Started

This file covers how to get started with `vertd`.

- [Downloading the server binaries](#downloading-the-server-binaries)
- [Running `vertd` on macOS/Linux](#running-vertd-on-macoslinux)
  - [Using systemd](#using-systemd)
  - [Using Docker](#using-docker)

## Downloading the server binaries

Grab the latest `vertd` release for your operating system and architecture from [this page](https://github.com/VERT-sh/vertd/releases).

> [!NOTE]
> If you're using an Intel-based Mac, download the `vertd-mac-x86_64` executable. For Mac computers with Apple silicon (M1 or newer), download `vertd-mac-arm64` instead.

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

You can also try opening http://localhost:24153 in your favorite web browser.

### Using Docker

Check out the [Docker Setup](./DOCKER_SETUP.md) page.
