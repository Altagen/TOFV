#!/bin/sh
# Fail unless every manifest agrees with the version being released.
#
# Worth its own script because `tofv --version` now reports the Cargo version:
# tagging 0.2.0 against a tree that still says 0.1.0 would publish a binary
# that lies about which release it is, and a PKGBUILD that fetches the wrong
# tarball.
#
#   ./scripts/check-version.sh 0.1.0
set -eu

want="${1:?usage: check-version.sh <version>}"
# A pre-release tag (0.1.0-rc.1) is built from the 0.1.0 tree.
base=${want%%-*}
ROOT=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
status=0

check() {
    label=$1
    got=$2
    if [ "$got" = "$base" ]; then
        printf '  ok    %-22s %s\n' "$label" "$got"
    else
        printf '  WRONG %-22s %s (expected %s)\n' "$label" "${got:-<none>}" "$base"
        status=1
    fi
}

check "Cargo.toml" \
    "$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)"
check "tauri.conf.json" \
    "$(sed -n 's/.*"version": "\(.*\)".*/\1/p' "$ROOT/src-tauri/tauri.conf.json" | head -1)"
check "ui/package.json" \
    "$(sed -n 's/.*"version": "\(.*\)".*/\1/p' "$ROOT/ui/package.json" | head -1)"
check "PKGBUILD pkgver" \
    "$(sed -n 's/^pkgver=\(.*\)/\1/p' "$ROOT/packaging/arch/PKGBUILD" | head -1)"

if [ "$status" -ne 0 ]; then
    echo
    echo "Version mismatch: bump every manifest to $base before tagging $want." >&2
fi
exit "$status"
