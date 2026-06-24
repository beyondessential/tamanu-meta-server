# Canopy

[Tamanu](https://www.bes.au/products/tamanu/) is an open-source patient-level electronic health records system for mobile and desktop.

Canopy provides:
- a server discovery service for the Tamanu mobile app
- the full list of available versions of Tamanu
- download URLs to available artifacts for active versions

and, internally:
- a global view of server status and healthchecks
- backup management
- associated tooling (bestool)

This is not expected to be usefully run by any other organisation; as a public-interest
non-profit, BES International publishes almost all of its software as open-source.

## Get

We have a container image for linux/amd64 and linux/arm64:

```
ghcr.io/beyondessential/canopy:latest
```

Each push to `main` builds and publishes a new image (also tagged
`sha-<short>` for the source commit) and triggers a pulumi deploy.

## Develop

- Install [Rustup](https://rustup.rs/), which will install Rust and Cargo.
- Install [just](https://just.systems/) command runner
- Clone the repo via git:

```console
$ git clone git@github.com:beyondessential/canopy.git
```

- Install development dependencies:

```console
$ just install-deps
```

This will install [cargo-nextest](https://nextest.rs),
[diesel CLI](https://diesel.rs/guides/getting-started.html#installing-diesel-cli),
and [watchexec](https://github.com/watchexec/watchexec).

### Quick Start

- Create a new blank postgres database.
- Optionally set the `DATABASE_URL` environment variable (if your database isn't named the default `canopy`):

```console
$ export DATABASE_URL=postgres://localhost/canopy_dev
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
$ just download-db canopy canopy-prod
```

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

### MCP

Claude Code:

```console
$ claude mcp add --transport http canopy https://canopy.tail53aef.ts.net/api/mcp
```

Then ask it things like "list the servers in group X" or "which backups are overdue".

