# Compiling from source

This file covers how to compile `vertd` from source.

- [Prerequisites](#prerequisites)
- [Cloning the repository](#cloning-the-repository)
- [Compiling](#compiling)

### Prerequisites

- Git
- A working Rust toolchain

### Cloning the repository

To clone the repository, run:

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

You might also run it by using `cargo run` instead.
