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
ghcr.io/beyondessential/tamanu-meta:5.1.2
```

## Develop

- Install [Rustup](https://rustup.rs/), which will install Rust and Cargo.
- Install [cargo-nextest](https://nextest.rs/)
- Install [cargo-leptos](https://leptos.dev/)
- Install [the diesel CLI tool](https://diesel.rs/guides/getting-started.html#installing-diesel-cli)
- Clone the repo via git:

```console
$ git clone git@github.com:beyondessential/tamanu-meta-server.git
```

- Build the project:

```console
$ cargo check
```

- Create a new blank postgres database.
- Set the `DATABASE_URL` environment variable.
  You can do that per diesel command, or for your entire shell session using `export` (or `set -x` in fish, or `$env:DATABASE_URL =` in powershell) as usual for your preferred shell.

- Run migrations:

```console
$ diesel migration run
```

- Run (public server and other binaries):

```console
$ cargo run
```

- Run (private server):

```console
$ cargo leptos watch
```

- Tests:

```console
$ cargo nextest run
```

You'll also need these environment variables for the private-server tests:
- `LEPTOS_OUTPUT_NAME=private-server`
- `SERVER_FN_MOD_PATH=true`
- `DISABLE_SERVER_FN_HASH=true`

We recommend using [Rust Analyzer](https://rust-analyzer.github.io/) or [Rust Rover](https://www.jetbrains.com/rust/) for development.

### Migrations

2. Create a migration
```console
$ diesel migration generate some_name_here
```

3. Write the migration's `up.sql` and `down.sql`

4. Run the pending migrations:
```console
$ diesel migration run
```

5. Test your down:
```console
$ diesel migration redo
```

6. Run formatter:
```console
$ cargo fmt
```

### Download a database

```console
kubectl exec -n tamanu-meta-dev meta-db-1 -c postgres -- pg_dump -Fc -d app > dev.dump
createdb tamanu_meta_dev
pg_restore --no-owner --role=$USER -d tamanu_meta_dev --verbose < dev.dump
```

### Releasing

(You need write access to the main branch directly)

On the main branch:

```console
$ cargo release --workspace --execute minor // or patch, major
```

Install `cargo-release` with:

```console
$ cargo install cargo-release
```

Also install `git-cliff`:

```console
$ cargo install git-cliff
```

### Public API Authentication

The `public-server` binary serves the public API and views, which are expected to be exposed to
the internet (in production behind an ingress gateway or reverse proxy).

The `mtls-certificate` (or `ssl-client-cert`) header should contain a PEM-encoded (optionally URL-encoded) X509 certificate.

To get a certificate, run:

```console
$ cargo run --bin identity
```

Which will write the `identity.crt.pem` and `identity.key.pem`.

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
