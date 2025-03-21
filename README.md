# Tamanu Meta Server

[Tamanu](https://www.bes.au/products/tamanu/) is an open-source patient-level electronic health records system for mobile and desktop.

The Meta service provides:
- a server discovery service for the Tamanu mobile app
- a server list and health check page
- a range of active versions

## Get

We have a container image for linux/amd64 and linux/arm64:

```
ghcr.io/beyondessential/tamanu-meta:2.5.1
```

## API

### Authentication

Routes marked with 🔐 require authentication.

The `mtls-certificate` header should contain a PEM-encoded (optionally URL-encoded) X509 certificate.

In production, the header should be set from a client certificate, as terminated by a reverse proxy or load balancer, and any matching header on the incoming requests should be stripped.

- Nginx: use the `$ssl_client_escaped_cert` variable.
- Caddy: use the `{http.request.tls.client.certificate_pem}` placeholder.

Alternatively, Rocket can be configured to terminate TLS itself, and handles the client certificate itself directly.
In this case, the certificate must be signed by the provided CA to pass validation.

### GET `/servers`

Get the full list of servers as JSON.

```json
[
	{
		"id":"8960470f-5282-496e-86f5-21df8cf67d62",
		"name":"Dev (main)",
		"host":"https://central.main.internal.tamanu.io/",
		"rank":"dev"
	}
]
```

### POST `/servers` 🔐

Add a server to the list.

Pass a JSON body:

```json
{
	"name":"Dev (main)",
	"host":"https://central.main.internal.tamanu.io/",
	"rank":"dev"
}
```

Returns the server with its assigned ID:

```json
{
	"id":"8960470f-5282-496e-86f5-21df8cf67d62",
	"name":"Dev (main)",
	"host":"https://central.main.internal.tamanu.io/",
	"rank":"dev"
}
```

### PATCH `/servers` 🔐

Edit a server.

Pass a JSON body, all fields optional except for `id`:

```json
{
	"id":"8960470f-5282-496e-86f5-21df8cf67d62",
	"name":"Test server (main)"
}
```

Returns the edited server with its assigned ID:

```json
{
	"id":"8960470f-5282-496e-86f5-21df8cf67d62",
	"name":"Test server (main)",
	"host":"https://central.main.internal.tamanu.io/",
	"rank":"dev"
}
```

There's a "hidden" feature in the UI where if you Shift-click a row, it will
copy its ID to the clipboard.

### DELETE `/servers` 🔐

Remove a server from the list.

Pass a JSON body with the `id` field:

```json
{
	"id":"8960470f-5282-496e-86f5-21df8cf67d62"
}
```

### POST `/reload` 🔐

Force a reload of the statuses.

### GET `/versions`

Returns the range of versions being used in production.

```json
{
	"min": "2.1.0",
	"max": "2.3.4"
}
```

## Develop

- Install [Rustup](https://rustup.rs/), which will install Rust and Cargo.
- Clone the repo via git:

```bash
$ git clone git@github.com:beyondessential/tamanu-meta-server.git
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

### Migrations

1. Install the diesel CLI tool: <https://diesel.rs/guides/getting-started.html#installing-diesel-cli>

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

### Releasing

On the main branch:

```console
$ cargo release --execute minor // or patch, major
```

Install `cargo-release` with:

```console
$ cargo install cargo-release
```
