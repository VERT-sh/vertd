# Getting Started

This file covers how to get started with `vertd`.

- [As a user](#as-a-user)
  - [Running it on macOS/Linux](#running-it-on-macoslinux)
  - [Running it under systemd](#using-systemd)
- [As a developer](#as-a-developer)
  - [Prerequisites](#prerequisites)
  - [Cloning](#cloning)
  - [Compiling](#compiling)

## As a user

Grab the latest `vertd` release for your operating system from [this page](https://github.com/VERT-sh/vertd/releases), then run it.

> [!NOTE]
> If you're using an Intel-based Mac, download the `vertd-mac-x86_64` executable. For Mac computers with Apple silicon, download `vertd-mac-arm64` instead.

### Running it on macOS/Linux

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

---

## As a developer

This section covers how to get started with `vertd` as a developer.

### Prerequisites

- Git
- cargo

### Cloning

Run:

```shell
$ git clone https://github.com/VERT-sh/vertd
$ cd vertd/
```

### Compiling

You can compile `vertd` using cargo (Rust's package manager and build tool):

```shell
$ cargo build           # for a debug build
$ cargo build --release # for a release build
```
