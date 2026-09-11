# Contributing to VERT

Thank you for taking your time to contribute to the `VERT-sh/vertd` repository! VERT uses many other FOSS projects and would not exist without the open source community <3

Below is our guidelines for contributing to this repository, but also note our general contributing guides found here: [CONTRIBUTING.md](https://github.com/VERT-sh/.github/blob/main/profile/CONTRIBUTING.md)

## Conventions

> For all contributions, please note our AI / LLM policy found on our general contributing guide: [CONTRIBUTING.md](https://github.com/VERT-sh/.github/blob/main/profile/CONTRIBUTING.md)

### Code changes

Clone the repository and create a branch for your changes:

```sh
git clone https://github.com/VERT-sh/vertd && cd vertd
cp .env.example .env
git switch -c your-branch
```

..then compile or run a development build with `cargo build` or `cargo run`.

Before submitting a pull request, run Cargo's formatter and linter - you may need to install these if they aren't already with `rustup component add rustfmt clippy`:

```sh
cargo fmt
cargo clippy
```

Please follow these conventions when contributing:

- Git commit messages should follow the [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/) standard
- Use `cargo fmt` to format your code
- Use `cargo clippy` to check your code
- Open your pull request after `cargo fmt` and `cargo lint` pass

### Issues / bugs

To report an issue, either join the VERT Discord server above and open a forum post in the server, or open a GitHub issue on the repository.

Please provide any relevant details, reproduction steps, and screenshots for us to replicate your issue. You may also provide the original file if you are comfortable to do so, publicly or privately (to our team through DMs) - see the general [CONTRIBUTING.md](https://github.com/VERT-sh/.github/blob/main/profile/CONTRIBUTING.md) to contact us.

### Documentation

Official documentation for self-hosting, development, and other frequently asked questions are stored in the `/docs` folder of the repo. Our documentation is currently written in English.
