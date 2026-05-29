#!/usr/bin/env bash
# Run a command against a throwaway, RAM-backed PostgreSQL instance.
#
# Why: the test suite creates and drops a fresh database (plus runs every
# migration) for *each* of its hundreds of tests, dozens in parallel. Against a
# disk-backed cluster the CREATE DATABASE / DROP DATABASE fsync storm saturates
# the machine's I/O and makes the whole system unresponsive. This spins up a
# disposable cluster on tmpfs with durability turned off, so none of that churn
# ever reaches a physical disk.
#
# This is opt-in (you need RAM to spare) and self-contained: it uses the
# `initdb`/`pg_ctl` already on your machine, so the server version matches your
# system Postgres exactly and there's no container runtime or image to manage.
#
# Usage:
#   scripts/ramdisk-pg.sh cargo nextest run -p database
#   scripts/ramdisk-pg.sh just test
#
# Env overrides:
#   CANOPY_TEST_PG_DIR    data directory (default: a fresh dir under /dev/shm)
#   CANOPY_TEST_PG_PORT   starting TCP port to probe (default: 5433)
#   CANOPY_TEST_PG_ROLE   superuser role / db owner (default: canopy)
set -euo pipefail

if [ "$#" -eq 0 ]; then
	echo "usage: $0 <command> [args...]" >&2
	exit 64
fi

ROLE="${CANOPY_TEST_PG_ROLE:-canopy}"

# Locate the Postgres server binaries. They're on PATH on Arch, but other
# installs hide them: Debian/Ubuntu under a versioned dir, Homebrew under
# opt/postgresql@NN, and Postgres.app inside its bundle. Fall back to those.
if ! command -v initdb >/dev/null 2>&1; then
	for d in \
		/usr/lib/postgresql/*/bin \
		/opt/homebrew/opt/postgresql*/bin \
		/usr/local/opt/postgresql*/bin \
		/Applications/Postgres.app/Contents/Versions/*/bin; do
		if [ -x "$d/initdb" ]; then
			PATH="$d:$PATH"
			break
		fi
	done
fi
for bin in initdb pg_ctl createdb; do
	command -v "$bin" >/dev/null 2>&1 || {
		echo "error: '$bin' not found on PATH. Install the Postgres server tools." >&2
		exit 69
	}
done

# Pick a tmpfs-backed data directory. /dev/shm is tmpfs on Linux; if it's
# missing (e.g. macOS) fall back to $TMPDIR but warn that it won't be RAM-backed.
if [ -n "${CANOPY_TEST_PG_DIR:-}" ]; then
	DATADIR="$CANOPY_TEST_PG_DIR"
	mkdir -p "$DATADIR"
	OWN_DATADIR=0
else
	if [ -d /dev/shm ] && [ -w /dev/shm ]; then
		BASE=/dev/shm
	else
		# No tmpfs by default on macOS. We still turn fsync off below, which is
		# what actually removes the I/O grind, so this stays fast — it just lands
		# on disk. For a true ramdisk, create one and pass CANOPY_TEST_PG_DIR,
		# e.g. macOS: diskutil erasevolume APFS canopy-pg $(hdiutil attach -nomount ram://1048576)
		BASE="${TMPDIR:-/tmp}"
		echo "note: no /dev/shm; using $BASE (disk-backed, but fsync is off so still fast)" >&2
	fi
	DATADIR="$(mktemp -d "$BASE/canopy-test-pg.XXXXXX")"
	OWN_DATADIR=1
fi

# Find a free TCP port, starting from the requested one.
port_in_use() { (exec 3<>"/dev/tcp/127.0.0.1/$1") 2>/dev/null; }
PORT="${CANOPY_TEST_PG_PORT:-5433}"
for _ in $(seq 0 20); do
	port_in_use "$PORT" || break
	PORT=$((PORT + 1))
done
if port_in_use "$PORT"; then
	echo "error: no free port found near ${CANOPY_TEST_PG_PORT:-5433}" >&2
	exit 69
fi

STARTED=0
cleanup() {
	status=$?
	if [ "$STARTED" = 1 ]; then
		pg_ctl -D "$DATADIR" -m immediate stop >/dev/null 2>&1 || true
	fi
	if [ "${OWN_DATADIR:-0}" = 1 ]; then
		rm -rf "$DATADIR"
	fi
	exit "$status"
}
trap cleanup EXIT INT TERM

echo "ramdisk-pg: initialising disposable cluster in $DATADIR (port $PORT)" >&2
# --no-sync: don't fsync the freshly-created cluster; it's disposable.
initdb -D "$DATADIR" -U "$ROLE" --auth=trust --no-sync -E UTF8 >/dev/null

# Durability-off settings are safe here precisely because the data is thrown
# away. max_connections is bumped well past Postgres's default of 100 to absorb
# the parallel test pools. unix_socket_directories points at the data dir so a
# stray socket never lands in a shared /tmp or /run.
pg_ctl -D "$DATADIR" -l "$DATADIR/postmaster.log" -w start -o "\
	-p $PORT \
	-h 127.0.0.1 \
	-k $DATADIR \
	-c fsync=off \
	-c synchronous_commit=off \
	-c full_page_writes=off \
	-c autovacuum=off \
	-c max_connections=300" >/dev/null
STARTED=1

createdb -h 127.0.0.1 -p "$PORT" -U "$ROLE" "$ROLE"

export DATABASE_URL="postgresql://${ROLE}@127.0.0.1:${PORT}/${ROLE}"
export RO_DATABASE_URL="$DATABASE_URL"
# So `just test-e2e` (whose fixture creates its own per-worker databases) can
# ride on the same RAM-backed cluster when invoked through this wrapper.
export CANOPY_E2E_ADMIN_DATABASE_URL="postgresql://${ROLE}@127.0.0.1:${PORT}/postgres"

echo "ramdisk-pg: DATABASE_URL=$DATABASE_URL" >&2
"$@"
