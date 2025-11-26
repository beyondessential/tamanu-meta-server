# Tamanu Meta Server

[Tamanu](https://www.bes.au/products/tamanu/) is an open-source patient-level electronic health records system for mobile and desktop.

The Meta service provides:
- a server discovery service for the Tamanu mobile app
- a server list and health check page
- a range of active versions
- download URLs to available artifacts for active versions

## Get

We have a container image for linux/amd64 and linux/arm64:

```
ghcr.io/beyondessential/tamanu-meta:5.7.3
```

## Develop

- Install [Rustup](https://rustup.rs/), which will install Rust and Cargo.
- Install [just](https://just.systems/) command runner
- Clone the repo via git:

```console
$ git clone git@github.com:beyondessential/tamanu-meta-server.git
```

- Install development dependencies:

```console
$ just install-deps
```

This will install [cargo-nextest](https://nextest.rs), [cargo-leptos](https://leptos.dev),
[diesel CLI](https://diesel.rs/guides/getting-started.html#installing-diesel-cli),
[cargo-release](https://github.com/crate-ci/cargo-release), [git-cliff](https://git-cliff.org),
and [watchexec](https://github.com/watchexec/watchexec).

### Quick Start

- Create a new blank postgres database.
- Optionally set the `DATABASE_URL` environment variable (if your database isn't named the default `tamanu_meta`):

```console
$ export DATABASE_URL=postgres://localhost/tamanu_meta_dev
```

- Run migrations:

```console
$ just migrate
```

- Build the project:

```console
$ just check
```

- Run public server:

```console
$ cargo watch-public
```

- Run private server:

```console
$ just watch-private
```

- Run other binaries:

```console
$ cargo run --bin binary_name_here
```

- Tests:

```console
$ just test
```

- Lints:

```console
$ just lint
```

- Format, lint, and test in one command:

```console
$ just dev
```

### Available Commands

See all available commands:

```console
$ just --list
```

We recommend using [Rust Analyzer](https://rust-analyzer.github.io/) or [Rust Rover](https://www.jetbrains.com/rust/) for development.

### Migrations

1. Create a migration:
```console
$ just migration some_name_here
```

2. Write the migration's `up.sql` and `down.sql`

3. Run the pending migrations:
```console
$ just migrate
```

4. Test your down:
```console
$ just migrate-redo
```

### Download a database

You'll need to have `kubectl` installed and authorised.

```console
# just download-db {database name} {kubernetes namespace} [dump file]
$ just download-db tamanu_meta tamanu-meta-prod
```

### Releasing

(You need write access to the main branch directly)

On the main branch:

```console
$ just release minor
```

(or use `patch` or `major` instead of `minor`)

### Public API Authentication

The `public-server` binary serves the public API and views, which are expected to be exposed to
the internet (in production behind an ingress gateway or reverse proxy).

The `mtls-certificate` (or `ssl-client-cert`) header should contain a PEM-encoded (optionally URL-encoded) X509 certificate.

To get a certificate, run:

```console
$ just identity
```

This will write the `identity.crt.pem` and `identity.key.pem`.

You can then put it in an environment variable:

```console
$ export MTLS_CERT="$(jq -sRr @uri identity.crt.pem)"
```

and then use curl like:

```console
$ curl -H "mtls-certificate: $MTLS_CERT" ...
```

#### In production

In production, the header should be set from a client certificate, as terminated by a reverse proxy or load balancer, and any matching header on the incoming requests should be stripped.

- Nginx: use the `$ssl_client_escaped_cert` variable.
- Caddy: use the `{http.request.tls.client.certificate_pem}` placeholder.
