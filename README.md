# Tamanu Meta Server

[Tamanu](https://www.bes.au/products/tamanu/) is an open-source patient-level electronic health records system for mobile and desktop.

The Meta service provides:
- a server discovery service for the Tamanu mobile app
- a server list and health check page
- a list of available versions

## Get

We have a container image for linux/amd64 and linux/arm64:

```
ghcr.io/beyondessential/tamanu-meta:2.3.2
```

## API

### Authentication

Routes marked with 🔐 require authentication, which is achieved by presenting a client TLS certificate.

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

### GET `/version/<version>`

Not yet implemented.

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
