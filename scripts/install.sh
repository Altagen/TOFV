#!/bin/sh
# One-shot local install from a git checkout (path 2 in README.md).
# Release binaries + helper/Polkit + .desktop + ~/.local/bin.
# Does not install distro packages (openfortivpn, ppp, libsecret) — the
# doctor screen / `tofv doctor` prints the right pacman/apt line.
# Do not run this after `pacman -S tofv` (path 1). Prebuilt tarball is path 3.
set -eu

ROOT=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
PROFILE="${TOFV_PROFILE:-release}"
export TOFV_BIN_DIR="$ROOT/target/$PROFILE"

echo "==> build ($PROFILE)"
if [ "${SKIP_BUILD:-}" != "1" ]; then
    if [ "$PROFILE" = "release" ]; then
        "$ROOT/scripts/build-app.sh" --release
        "$ROOT/scripts/cargo.sh" build --release -p tofv-helper -p pinentry-tofv
        "$ROOT/scripts/cargo.sh" build --release -p tofv-core --bin tofv
    else
        "$ROOT/scripts/build-app.sh"
        "$ROOT/scripts/cargo.sh" build -p tofv-helper -p pinentry-tofv
        "$ROOT/scripts/cargo.sh" build -p tofv-core --bin tofv
    fi
fi

echo "==> helper + Polkit (sudo)"
"$ROOT/scripts/install-helper.sh"

echo "==> user launcher"
"$ROOT/scripts/install-desktop.sh"
mkdir -p "$HOME/.local/bin"
if [ -x "$TOFV_BIN_DIR/tofv" ]; then
    ln -sfn "$TOFV_BIN_DIR/tofv" "$HOME/.local/bin/tofv"
fi

echo
echo "TOFV is in ~/.local/bin/tofv-app  (reload PATH if needed)"
echo
if [ -x "$TOFV_BIN_DIR/tofv" ]; then
    "$TOFV_BIN_DIR/tofv" doctor || true
fi
echo
echo "Run (the terminal does not need to stay open):"
echo "  tofv-app                 # panel + tray, detaches from the TTY"
echo "  tofv-app --tray          # icon only (login / autostart)"
echo "  tofv-app --foreground    # logs in this terminal"
echo "Or double-click TOFV in your applications menu."
