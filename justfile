# Tamanu Meta Server Development Commands

# Default database URL (can be overridden)
DATABASE_URL := env_var_or_default('DATABASE_URL', 'postgres://localhost/tamanu_meta')

# Environment variables for Leptos tests
export LEPTOS_OUTPUT_NAME := "private-server"
export SERVER_FN_MOD_PATH := "true"
export DISABLE_SERVER_FN_HASH := "true"

# Show available commands
default:
    @just --list

# Check if the project compiles
check:
    cargo check

# Build the project Docker image
build-image:
    docker build -t tamanu-meta-server .

# Run the public server and reload on change
watch-public: _copy-bulma
	watchexec -w crates -E SERVER_VERSIONS_SECRET=test -- cargo run --bin public-server

# Run the private server with live reload
watch-private: _copy-bulma
    cargo leptos watch

# Run all tests
test:
    DATABASE_URL={{DATABASE_URL}} cargo nextest run

# Run tests for a specific package
test-package package:
    DATABASE_URL={{DATABASE_URL}} cargo nextest run -p {{package}}

# Run a specific test
test-name name:
    DATABASE_URL={{DATABASE_URL}} cargo nextest run {{name}}

# Run tests with no capture (show output)
test-verbose:
    DATABASE_URL={{DATABASE_URL}} cargo nextest run --no-capture

# Run migrations
migrate:
    DATABASE_URL={{DATABASE_URL}} diesel migration run
    cargo fmt

# Create a new migration
migration name:
    DATABASE_URL={{DATABASE_URL}} diesel migration generate {{name}}

# Redo the last migration (down then up)
migrate-redo:
    DATABASE_URL={{DATABASE_URL}} diesel migration redo
    cargo fmt

# Revert the last migration
migrate-revert:
    DATABASE_URL={{DATABASE_URL}} diesel migration revert
    cargo fmt

# Format code
fmt:
    cargo fmt
    leptosfmt crates/private-server/**/*.rs

# Check formatting without making changes
fmt-check:
    cargo fmt --check

# Run clippy lints
lint:
	cargo clippy --all-features --all-targets

# Fix clippy warnings automatically where possible
lint-fix:
	cargo clippy --all-features --all-targets --fix --allow-dirty --allow-staged

# Generate identity certificate for API authentication
identity:
    cargo run --bin identity

# Clean build artifacts
clean:
    cargo clean

# Build server binaries for a specific target (release mode)
build-servers-release target: _copy-bulma
	cargo build --locked --target {{target}} --release --bins

# Detect and cache wasm-bindgen version
_bindgen-version:
	#!/usr/bin/env bash
	set -euo pipefail
	mkdir -p target
	if [ ! -f target/wasm-bindgen-version ]; then
		cargo metadata --format-version 1 --locked | jq -r 'first(.packages[] | select(.name=="wasm-bindgen") | .version)' > target/wasm-bindgen-version
	fi

# Build the frontend only (private server)
build-frontend: _bindgen-version _copy-bulma
	#!/usr/bin/env bash
	set -x
	export LEPTOS_WASM_BINDGEN_VERSION=$(cat target/wasm-bindgen-version)
	cargo leptos build --frontend-only

# Build the frontend for production (with compression)
build-frontend-release: _bindgen-version _copy-bulma
	#!/usr/bin/env bash
	set -x
	export LEPTOS_WASM_BINDGEN_VERSION=$(cat target/wasm-bindgen-version)
	cargo leptos build --release --frontend-only --precompress

# Install development dependencies
install-deps:
	cargo binstall -y cargo-binstall || cargo install cargo-binstall
	cargo binstall -y cargo-nextest cargo-leptos leptosfmt cargo-release git-cliff watchexec-cli diesel_cli

# Download database from Kubernetes
download-db dbname namespace="tamanu-meta-dev" pod="meta-db-1" output="app.dump":
	dropdb {{dbname}} || true
	createdb {{dbname}} || true
	kubectl exec -n {{namespace}} {{pod}} -c postgres -- pg_dump -Fc -d app > {{output}}
	pg_restore --no-owner --role=$USER -d {{dbname}} --verbose < {{output}}

# Development cycle: format, lint, test
dev: fmt lint test

# Make a new release
release level="minor":
	cargo release --workspace --execute {{level}}

# Update the bulma submodule
update-bulma:
	git submodule update --init --recursive
	git submodule foreach git pull origin main

# Copy bulma CSS files to static directory
_copy-bulma:
	cp -r --reflink=auto .sub/bulma/css static/bulma
