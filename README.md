# Tamanu Meta Server

[Tamanu](https://www.bes.au/products/tamanu/) is an open-source patient-level electronic health records system for mobile and desktop.

The Meta service provides:
- a server discovery service for the Tamanu mobile app
- a server list and health check page
- a list of available versions

## Get

We have a container image for linux/amd64 and linux/arm64:

```
ghcr.io/beyondessential/tamanu-meta:v2.0.1
```

## API

TODO

## Develop

- Install [Rustup](https://rustup.rs/), which will install Rust and Cargo.
- Clone the repo via git:

```bash
$ git clone git@github.com:beyondessential/tamanu.git
```

- Build the project:

```bash
$ cargo check
```

- Run with:

```bash
$ cargo run
```

- Tests:

```bash
$ cargo test
```

We recommend using [Rust Analyzer](https://rust-analyzer.github.io/) or [Rust Rover](https://www.jetbrains.com/rust/) for development.
