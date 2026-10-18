#!/bin/sh
# Keeps target/ from piling up stale build artifacts: after a successful build, runs cargo clean
# if target/ is over the cap. Cap in GB from TARGET_CAP_GB, default 10.
cd "$(dirname "$0")/../.." || exit 1
cap_gb=${TARGET_CAP_GB:-10}
[ -d target ] || exit 0
size_kb=$(du -sk target | cut -f1)
if [ "$size_kb" -gt $((cap_gb * 1024 * 1024)) ]; then
  echo "target/ is $((size_kb / 1024 / 1024)) GB, over the ${cap_gb} GB cap: running cargo clean"
  cargo clean
fi
