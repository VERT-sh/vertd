# Getting Started

This file covers how to get started with `vertd`.

- [As a user](#as-a-user)
  - [Running it on macOS/Linux](#running-it-on-macoslinux)
- [As a developer](#as-a-developer)
  - [Prerequisites](#prerequisites)
  - [Cloning](#cloning)
  - [Compiling](#compiling)
  - [Running](#running)

## As a user

Grab the latest `vertd` release for your operating system from [this page](https://github.com/VERT-sh/vertd/releases), then run it.

### Running it on macOS/Linux

Assuming you downloaded the `vertd` executable to your Downloads folder, open the Terminal and run the following command to navigate there:

```shell
$ cd ~/Downloads/
```

Then, modify the permissions of the executable and run it by using:

```shell
$ chmod +x vertd-os-arch
$ ./vertd-os-arch
```

You should modify `vertd-os-arch` to be the name of the executable you downloaded earlier (for example, on an Apple silicon Mac this would be `vertd-mac-arm64`)

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

You can compile `vertd` using:

```shell
$ cargo build           # for a debug build
$ cargo build --release # for a release build
```

### Running

You can run `vertd` with cargo by using:

```shell
$ cargo run           # for a debug build
$ cargo run --release # for a release build
```
