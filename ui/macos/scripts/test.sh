#!/bin/bash
# swift test, plus the Testing macro plugin path that Command Line Tools installs don't pass on their own.
set -euo pipefail
cd "$(dirname "$0")/.."
parallel=1
for arg in "$@"; do
  if [[ "$arg" == --parallel || "$arg" == --no-parallel ]]; then
    parallel=0
    break
  fi
done
plugins="$(xcode-select -p)/usr/lib/swift/host/plugins/testing"
if [ -d "$plugins" ]; then
  if [ "$parallel" -eq 1 ]; then
    exec swift test --parallel -Xswiftc -plugin-path -Xswiftc "$plugins" "$@"
  fi
  exec swift test -Xswiftc -plugin-path -Xswiftc "$plugins" "$@"
fi
if [ "$parallel" -eq 1 ]; then
  exec swift test --parallel "$@"
fi
exec swift test "$@"
