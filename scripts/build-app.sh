#!/bin/sh
# Build the UI on the host (Node is already there), then the Tauri binary in Podman.
set -eu
ROOT=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
cd "$ROOT/ui"
if [ ! -d node_modules ]; then
    npm install
fi
npm run build
cd "$ROOT"
# Containerfile changed from Debian → Arch; always rebuild if asked.
if [ "${TOFV_REBUILD:-}" = "1" ] || ! podman image exists "${TOFV_IMAGE:-localhost/tofv-dev}"; then
    podman build -t "${TOFV_IMAGE:-localhost/tofv-dev}" -f "$ROOT/Containerfile" "$ROOT"
fi
exec "$ROOT/scripts/cargo.sh" build -p tofv-app "$@"
