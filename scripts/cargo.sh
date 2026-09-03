#!/bin/sh
# Run cargo inside the TOFV toolchain image (rootless podman).
set -eu

IMAGE="${TOFV_IMAGE:-localhost/tofv-dev}"
ROOT=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)

if ! podman image exists "$IMAGE"; then
    podman build -t "$IMAGE" -f "$ROOT/Containerfile" "$ROOT"
fi

# keep-id: artifacts in target/ belong to the host user.
# CARGO_HOME on the bind mount so uid-mapped cargo can write the registry.
mkdir -p "$ROOT/.cache/cargo" "$ROOT/target"

exec podman run --rm \
    --userns=keep-id \
    -v "$ROOT":/src:Z \
    -w /src \
    -e CARGO_HOME=/src/.cache/cargo \
    "$IMAGE" \
    cargo "$@"
