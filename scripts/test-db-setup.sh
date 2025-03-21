#!/usr/bin/env bash
set -euo pipefail

# TODO: where to store this info
: "${NEXTEST_ENV:=.env.test}"

# Extract database URL from Rocket.toml
DB_URL=$(grep -A1 'databases.postgres' Rocket.toml | grep 'url' | sed -E 's/.*"(.+)".*/\1/')

echo "Database URL: $DB_URL"

# Handle both postgresql:// and postgres:// prefixes
if [[ "$DB_URL" =~ ^postgresql://(.+):(.+)@(.+):([0-9]+)/(.+)$ ]]; then
    DB_USER="${BASH_REMATCH[1]}"
    DB_PASS="${BASH_REMATCH[2]}"
    DB_HOST="${BASH_REMATCH[3]}"
    DB_PORT="${BASH_REMATCH[4]}"
    DB_NAME="${BASH_REMATCH[5]}"
else
    echo "Error: Could not parse database URL. Expected format: postgresql://user:pass@host:port/dbname"
    exit 1
fi
# Create test database name
TEST_DB_NAME="${DB_NAME}_test"
BASE_URL="postgresql://${DB_USER}:${DB_PASS}@${DB_HOST}:${DB_PORT}"
TEST_DB_URL="${BASE_URL}/${TEST_DB_NAME}"

# Export password for psql
export PGPASSWORD="$DB_PASS"

echo "Setting up test database: ${TEST_DB_NAME}"

psql -h "$DB_HOST" -p "$DB_PORT" -U "$DB_USER" -d "postgres" -c "DROP DATABASE IF EXISTS \"${TEST_DB_NAME}\";"
psql -h "$DB_HOST" -p "$DB_PORT" -U "$DB_USER" -d "postgres" -c "CREATE DATABASE \"${TEST_DB_NAME}\";"

# Set the test database URL for tests to use
echo "TEST_DATABASE_URL=${TEST_DB_URL}" > "$NEXTEST_ENV"

# Export for nextest environment
echo "::nextest-env-var::TEST_DATABASE_URL=${TEST_DB_URL}"

# Run diesel migrations on the test database
DATABASE_URL="${TEST_DB_URL}" diesel migration run
