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
watch-public:
	watchexec -w crates -- cargo run --bin public-server

# Run the private server with live reload
watch-private:
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

# Build the frontend only (private server)
build-frontend:
    cargo leptos build --frontend-only

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
