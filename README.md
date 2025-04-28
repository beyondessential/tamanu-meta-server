# Tamanu Meta Server

[Tamanu](https://www.bes.au/products/tamanu/) is an open-source patient-level electronic health records system for mobile and desktop.

The Meta service provides:
- a server discovery service for the Tamanu mobile app
- a server list and health check page
- a range of active versions

## Get

We have a container image for linux/amd64 and linux/arm64:

```
ghcr.io/beyondessential/tamanu-meta:3.3.0
```

## API

### Authentication

Routes marked with 🔐 require authentication; the word in (parens) after the emoji is the required `role`; `admin` role can do everything.

The `mtls-certificate` (or `ssl-client-cert`) header should contain a PEM-encoded (optionally URL-encoded) X509 certificate.
Alternatively, Rocket can be configured to terminate TLS itself, and handles the client certificate itself directly.
In this case, the certificate must be signed by the provided CA (enabled by `default.tls.mutual`) to pass validation; you'll need to generate a CA with [smallstep CLI](https://smallstep.com/docs/step-cli/) in that case, this is not covered here.

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

#### Roles

When you first connect to an authenticated API with a certificate, you'll get a 403.
Your public key will be added to the `devices` table.

Open the database, e.g. with PSQL, and change the role to `admin` (or as required).

```sql
UPDATE devices SET role = 'admin' WHERE id = '45886aa8-dff3-4cf7-92a9-31f42d4a0e1a';
```

In production, you need to do extra checks.

- Show untrusted devices with their public key in PEM-ish format:

```sql
SELECT id, created_at, encode(key_data, 'base64') as pem
FROM devices WHERE role = 'untrusted'
ORDER BY created_at DESC \gx
```

- Compare the provided public key to the list to find the right `id`. You can also filter by the **last** few characters of the key (the first characters will all be the same):

```sql
SELECT id, created_at, encode(key_data, 'base64') as pem
FROM devices WHERE role = 'untrusted'
AND encode(key_data, 'base64') LIKE '%NWwjGDiHVWrBA=='
ORDER BY created_at DESC \gx
```

- Once you have the `id`, look at the connection metadata to see if it matches what you know for additional verification:

```sql
SELECT * FROM device_connections
WHERE id = '45886aa8-dff3-4cf7-92a9-31f42d4a0e1a'
ORDER BY created_at DESC \gx
```

#### In production

In production, the header should be set from a client certificate, as terminated by a reverse proxy or load balancer, and any matching header on the incoming requests should be stripped.

- Nginx: use the `$ssl_client_escaped_cert` variable.
- Caddy: use the `{http.request.tls.client.certificate_pem}` placeholder.

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

### POST `/servers` 🔐 (server)

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

### PATCH `/servers` 🔐 (admin)

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

### DELETE `/servers` 🔐 (admin)

Remove a server from the list.

Pass a JSON body with the `id` field:

```json
{
	"id":"8960470f-5282-496e-86f5-21df8cf67d62"
}
```

### POST `/reload` 🔐 (admin)

Force a reload of the statuses.

### GET `/versions`

Get a list of known Tamanu versions in JSON.

```js
[
	{
		"id": "967c7d15-7046-459e-a448-6584f72e55ce",
		"major": 2,
		"minor": 27,
		"patch": 0,
		"published": true,
		"changelog": "..."
	},
	// ...
]
```

### POST `/versions/<version>` 🔐 (releaser)

Create the version `<version>` (must be a semver version string).

Pass the changelog as the body (if using curl, use `--data-binary` to preserve whitespace):

```text
Blah
blah

blah
```

Returns the version with its assigned ID:

```json
{
	"id": "967c7d15-7046-459e-a448-6584f72e55ce",
	"major": 2,
	"minor": 27,
	"patch": 0,
	"published": true,
	"changelog": "..."
}
```

### DELETE `/versions/<version>` 🔐 (admin)

TO BE DOCUMENTED

### GET `/versions/<version>/artifacts`

TO BE DOCUMENTED

### GET `/versions/update-for/<version>`

TO BE DOCUMENTED

## Develop

- Install [Rustup](https://rustup.rs/), which will install Rust and Cargo.
- Install the diesel CLI tool: <https://diesel.rs/guides/getting-started.html#installing-diesel-cli>
- Clone the repo via git:

```console
$ git clone git@github.com:beyondessential/tamanu-meta-server.git
```

- Build the project:

```console
$ cargo check
```

- Create a new blank postgres database.
- Create a `Rocket.toml` from the `Rocket.example.toml`.
- Set the `DATABASE_URL` environment variable (for diesel).
  You can do that per diesel command, or for your entire shell session using `export` (or `set -x` in fish, or `$env:DATABASE_URL =` in powershell) as usual for your preferred shell.

- Run migrations:

```console
$ diesel migration run
```

- Run with:

```console
$ cargo run
```

- Tests:

```console
$ cargo test
```

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

### Releasing

(You need write access to the main branch directly)

On the main branch:

```console
$ cargo release --execute minor // or patch, major
```

Install `cargo-release` with:

```console
$ cargo install cargo-release
```

Also install `git-cliff`:

```console
$ cargo install git-cliff
```
