# Canopy Development Commands
# Default database URL (can be overridden)

export DATABASE_URL := env('DATABASE_URL', 'postgres://localhost/canopy')
export RO_DATABASE_URL := env('RO_DATABASE_URL', 'postgres://localhost/canopy')

# ...for development

export SERVER_VERSIONS_SECRET := "test"
export PUBLIC_URL := "http://localhost:8080"

# Skip the npm install + npm run build that private-server's build.rs runs by default.
# Set this in dev recipes — Vite serves the frontend directly there, and we don't
# need the binary to embed dist/ for cargo check / cargo run / cargo test.
export SKIP_FRONTEND_BUILD := "1"

# Show available commands
default:
    @just --list

# Check if the project compiles
check:
    cargo check

# Build the project Docker image
build-image:
    docker build -t canopy .

# Run the public server and reload on change
watch-public: _copy-bulma
    watchexec -w crates -- cargo run --bin public-server

# Rebuild the private-server binary on source change (pair with watch-private-api)
watch-private-build:
    watchexec -I -w crates -- cargo build --bin private-server

# Run the private server's HTTP API, bound to 127.0.0.1:8081, for the private-web Vite frontend.
# Watches the built binary so it restarts when watch-private-build produces a fresh artefact.
watch-private-api:
    BIND_ADDRESS=127.0.0.1:8081 watchexec -I -W target/debug -f private-server -- target/debug/private-server

# Run the private-web React frontend dev server (Vite proxy expects watch-private-api)
watch-private-web:
    cd private-web && npm run dev

# Run all tests
test:
    DATABASE_URL={{ DATABASE_URL }} cargo nextest run

# Run tests for a specific package
test-package package:
    DATABASE_URL={{ DATABASE_URL }} cargo nextest run -p {{ package }}

# Run a specific test
test-name name:
    DATABASE_URL={{ DATABASE_URL }} cargo nextest run {{ name }}

# Run tests with no capture (show output)
test-verbose:
    DATABASE_URL={{ DATABASE_URL }} cargo nextest run --no-capture

# Run migrations
migrate:
    DATABASE_URL={{ DATABASE_URL }} diesel migration run
    cargo fmt

# Create a new migration
migration name:
    DATABASE_URL={{ DATABASE_URL }} diesel migration generate {{ name }}

# Redo the last migration (down then up)
migrate-redo:
    DATABASE_URL={{ DATABASE_URL }} diesel migration redo
    cargo fmt

# Revert the last migration
migrate-revert:
    DATABASE_URL={{ DATABASE_URL }} diesel migration revert
    cargo fmt

# Format code
fmt:
    cargo fmt

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

# Build server binaries for a specific target (release mode), with embedded private-web frontend
build-servers-release target: _copy-bulma
    SKIP_FRONTEND_BUILD= cargo build --locked --target {{ target }} --release --bins

# Install development dependencies
install-deps:
    cargo binstall -y cargo-binstall || cargo install cargo-binstall
    cargo binstall -y cargo-nextest cargo-release git-cliff watchexec-cli diesel_cli

# Download database from Kubernetes
download-db dbname namespace="canopy-dev" pod="meta-db-1" output="app.dump":
    dropdb {{ dbname }} || true
    createdb {{ dbname }} || true
    kubectl exec -n {{ namespace }} {{ pod }} -c postgres -- pg_dump -Fc -d app > {{ output }}
    pg_restore --no-owner --role=$USER -d {{ dbname }} --verbose < {{ output }}

# Development cycle: format, lint, test
dev: fmt lint test

# Make a new release
release level="minor":
    cargo release --workspace --execute {{ level }}

# Update the bulma submodule
update-bulma:
    git submodule update --init --recursive
    git submodule foreach git pull origin main

# Copy bulma CSS files to static directory (used by the public-server templates)
_copy-bulma:
    cp -r --reflink=auto .sub/bulma/css static/bulma
