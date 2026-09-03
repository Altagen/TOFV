#!/bin/sh
# Install TOFV from an unpacked release tarball (path 3 in README.md).
#
# Same result as scripts/install.sh, without the build step: the binaries are
# already here. Run it from inside the unpacked directory.
#
# TOFV is only a wrapper — openfortivpn, pppd, a Secret Service keyring and
# polkit still have to come from your distribution. `tofv doctor` says which
# are missing and prints the command for your distro.
set -eu

ROOT=$(CDPATH= cd -- "$(dirname "$0")" && pwd)
APP_ID=dev.tofv
LIBEXEC="${TOFV_LIBEXEC:-/usr/local/libexec}"
POLICY_DEST=/usr/share/polkit-1/actions/${APP_ID}.policy
DATA="${XDG_DATA_HOME:-$HOME/.local/share}"
HICOLOR="$DATA/icons/hicolor"
APPS="$DATA/applications"
BINDIR="$HOME/.local/bin"
AUTOSTART="${XDG_CONFIG_HOME:-$HOME/.config}/autostart"

for f in tofv-app tofv tofv-helper pinentry-tofv; do
    [ -x "$ROOT/$f" ] || { echo "missing $f — is this a complete tarball?" >&2; exit 1; }
done

# --- root: the helper and its polkit rule -------------------------------
# This is the only privileged part. Everything else stays in $HOME.
echo "==> helper + polkit (sudo)"
sudo install -D -m 755 "$ROOT/tofv-helper" "$LIBEXEC/tofv-helper"
sudo install -D -m 755 "$ROOT/pinentry-tofv" "$LIBEXEC/pinentry-tofv"
sed "s|@LIBEXEC@|$LIBEXEC|g" "$ROOT/dev.tofv.policy.in" \
    | sudo install -D -m 644 /dev/stdin "$POLICY_DEST"

# --- user: binaries, launcher, icons ------------------------------------
echo "==> user launcher"
mkdir -p "$BINDIR" "$APPS" "$AUTOSTART"
install -m 755 "$ROOT/tofv-app" "$BINDIR/tofv-app"
install -m 755 "$ROOT/tofv" "$BINDIR/tofv"

for size in 32 64 128 256 512; do
    src="$ROOT/icons/mark-${size}.png"
    [ -f "$src" ] || continue
    dest="$HICOLOR/${size}x${size}/apps/${APP_ID}.png"
    mkdir -p "$(dirname "$dest")"
    install -m 644 "$src" "$dest"
    ln -sfn "${APP_ID}.png" "$HICOLOR/${size}x${size}/apps/tofv-app.png"
done

APP_BIN="$BINDIR/tofv-app"
sed -e "s|@TRYEXEC@|$APP_BIN|" -e "s|@EXEC@|$APP_BIN|" \
    "$ROOT/dev.tofv.desktop" > "$APPS/${APP_ID}.desktop"
sed "s|^StartupWMClass=.*|StartupWMClass=tofv-app|" "$APPS/${APP_ID}.desktop" \
    > "$APPS/tofv-app.desktop"
chmod 644 "$APPS/${APP_ID}.desktop" "$APPS/tofv-app.desktop"

{
    sed -e "s|@TRYEXEC@|$APP_BIN|" -e "s|@EXEC@|$APP_BIN --tray|" \
        "$ROOT/dev.tofv.desktop"
    echo "Hidden=false"
    echo "X-GNOME-Autostart-enabled=true"
} > "$AUTOSTART/${APP_ID}.desktop"
chmod 644 "$AUTOSTART/${APP_ID}.desktop"

command -v gtk-update-icon-cache >/dev/null 2>&1 && \
    gtk-update-icon-cache -f -t "$HICOLOR" 2>/dev/null || true
command -v update-desktop-database >/dev/null 2>&1 && \
    update-desktop-database "$APPS" 2>/dev/null || true
command -v kbuildsycoca6 >/dev/null 2>&1 && kbuildsycoca6 >/dev/null 2>&1 || true

echo
"$BINDIR/tofv" doctor || true
echo
echo "Run: tofv-app        (or double-click TOFV in your applications menu)"
