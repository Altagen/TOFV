#!/bin/sh
# Root install: helper + pinentry + Polkit rule.
# After this, Connect / Disconnect should not ask for the admin password.
set -eu

ROOT=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
DEST="${TOFV_LIBEXEC:-/usr/local/libexec}"
POLICY_DEST=/usr/share/polkit-1/actions/dev.tofv.policy
BINDIR="${TOFV_BIN_DIR:-}"

if [ -z "$BINDIR" ]; then
    if [ -x "$ROOT/target/release/tofv-helper" ]; then
        BINDIR="$ROOT/target/release"
    else
        BINDIR="$ROOT/target/debug"
    fi
fi

HELPER="$BINDIR/tofv-helper"
PIN="$BINDIR/pinentry-tofv"
if [ ! -x "$HELPER" ] || [ ! -x "$PIN" ]; then
    echo "building helper (release)…" >&2
    "$ROOT/scripts/cargo.sh" build --release -p tofv-helper -p pinentry-tofv
    BINDIR="$ROOT/target/release"
    HELPER="$BINDIR/tofv-helper"
    PIN="$BINDIR/pinentry-tofv"
fi

POLICY_IN="$ROOT/packaging/linux/polkit/dev.tofv.policy.in"
POLICY_TMP=$(mktemp)
sed "s|@LIBEXEC@|$DEST|g" "$POLICY_IN" > "$POLICY_TMP"

echo "installing $DEST/tofv-helper and pinentry-tofv (sudo)…"
sudo install -D -m 755 "$HELPER" "$DEST/tofv-helper"
sudo install -D -m 755 "$PIN" "$DEST/pinentry-tofv"
sudo install -D -m 644 "$POLICY_TMP" "$POLICY_DEST"
rm -f "$POLICY_TMP"

echo
echo "OK. Polkit: allow_active=yes for $DEST/tofv-helper"
echo "Connect / Disconnect should no longer ask for the admin password."
echo "Test: pkexec $DEST/tofv-helper stop"
