#!/usr/bin/env bash
# Launch an installed Sink headlessly and prove it stays up.
#
#   packaging/smoke-test.sh [command...]     (default: /usr/bin/sink)
#
# A healthy tray app runs its event loop until timeout kills it (exit 124).
# Any earlier exit is a real failure: a missing shared library (the loader
# aborts before main), a tray or webkit init failure, or a panic. Grepping
# the log for panics alone would let the first two pass.
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
