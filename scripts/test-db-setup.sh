#!/usr/bin/env bash
set -euo pipefail

# Extract database URL from Rocket.toml
DB_URL=$(grep -A1 'databases.postgres' Rocket.toml | grep 'url' | sed -E 's/.*"(.+)".*/\1/')

DB_URL_BASE="${DB_URL%/*}"
POSTGRES_DB_URL="${DB_URL_BASE}/postgres"

# Create test database name
TEST_DB_NAME="meta_server_test"
TEST_DB_URL="${DB_URL_BASE}/${TEST_DB_NAME}"

echo "Setting up test database: ${TEST_DB_NAME}" "${TEST_DB_URL}"

psql -d "$POSTGRES_DB_URL" -c "DROP DATABASE IF EXISTS \"${TEST_DB_NAME}\";"
psql -d "$POSTGRES_DB_URL" -c "CREATE DATABASE \"${TEST_DB_NAME}\";"

echo "ROCKET_DEFAULT_DATABASES_POSTGRES='{url=\"${TEST_DB_URL}\"}'" >> "$NEXTEST_ENV"

# Run diesel migrations on the test database
DATABASE_URL="${TEST_DB_URL}" diesel migration run
