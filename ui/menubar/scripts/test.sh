#!/bin/bash
# swift test, plus the Testing macro plugin path that Command Line Tools installs don't pass on their own.
set -euo pipefail
cd "$(dirname "$0")/.."
plugins="$(xcode-select -p)/usr/lib/swift/host/plugins/testing"
if [ -d "$plugins" ]; then
  exec swift test -Xswiftc -plugin-path -Xswiftc "$plugins" "$@"
fi
exec swift test "$@"
