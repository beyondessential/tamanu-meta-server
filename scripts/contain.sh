#!/usr/bin/env bash
# Run a command under resource limits so heavy builds and test runs can't
# freeze the machine.
#
# Why: linking test binaries and hammering a test database produce sustained
# write I/O and memory/page-cache pressure. On NVMe with the `none` scheduler,
# `ionice` is a no-op, and plain `nice` only helps with CPU — neither stops
# the kernel drowning in dirty pages. A cgroup does: MemoryHigh makes the
# group throttle and write back its own pages instead of evicting everything
# else, and CPUWeight keeps the desktop responsive under full compile load.
#
# Uses a transient systemd user scope when available (Linux). Falls back to
# plain `nice` elsewhere (macOS, containers without a user manager).
#
# Usage:
#   scripts/contain.sh cargo check
#   scripts/contain.sh scripts/ramdisk-pg.sh cargo nextest run
set -euo pipefail

if [ "$#" -eq 0 ]; then
	echo "usage: $0 <command> [args...]" >&2
	exit 64
fi

# Probe that a transient user scope actually works here; `command -v` alone
# isn't enough (e.g. CI containers ship systemd-run without a user manager).
if command -v systemd-run >/dev/null 2>&1 && systemd-run --user --scope -q true 2>/dev/null; then
	exec systemd-run --user --scope -q \
		-p MemoryHigh=70% \
		-p CPUWeight=40 \
		-p IOWeight=30 \
		nice "$@"
else
	exec nice "$@"
fi
