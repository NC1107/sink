#!/usr/bin/env bash
# Launches an installed Sink headlessly and proves it stays alive: any exit
# before the timeout is a real failure (missing library, init failure, panic).
set -uo pipefail

log=$(mktemp)
timeout 25 xvfb-run -a "${@:-/usr/bin/sink}" >"$log" 2>&1
code=$?
cat "$log"

if grep -qiE 'panicked at|error while loading shared libraries|cannot open shared object|symbol lookup error' "$log"; then
  echo "::error::sink failed to start cleanly - see the log above"
  exit 1
fi
if [ "$code" -ne 124 ]; then
  echo "::error::sink exited early (code $code) instead of staying alive for 25s"
  exit 1
fi
echo "sink launched and stayed alive under xvfb"
