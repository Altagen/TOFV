#!/bin/sh
# User-level icons + .desktop so Plasma/Wayland can replace the generic
# Wayland icon. No sudo. Tray still needs: sudo pacman -S libayatana-appindicator
set -eu

ROOT=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
APP_ID=dev.tofv
DATA="${XDG_DATA_HOME:-$HOME/.local/share}"
HICOLOR="$DATA/icons/hicolor"
APPS="$DATA/applications"
BINDIR="$HOME/.local/bin"
OUT="${TOFV_BIN_DIR:-}"
if [ -z "$OUT" ]; then
    if [ -x "$ROOT/target/release/tofv-app" ]; then
        OUT="$ROOT/target/release"
    else
        OUT="$ROOT/target/debug"
    fi
fi
BIN="$OUT/tofv-app"

if [ ! -x "$BIN" ]; then
    echo "pas de binaire $BIN — lance d’abord ./scripts/install.sh" >&2
    exit 1
fi

mkdir -p "$BINDIR" "$APPS"
ln -sfn "$BIN" "$BINDIR/tofv-app"

install_size() {
    size=$1
    src=$2
    dest="$HICOLOR/${size}x${size}/apps/${APP_ID}.png"
    mkdir -p "$(dirname "$dest")"
    install -m 644 "$src" "$dest"
    # fallback si l’app_id Wayland reste le nom du binaire
    ln -sfn "${APP_ID}.png" "$HICOLOR/${size}x${size}/apps/tofv-app.png"
}

install_size 32 "$ROOT/branding/mark-32.png"
install_size 64 "$ROOT/branding/mark-64.png"
install_size 128 "$ROOT/branding/mark-128.png"
install_size 256 "$ROOT/branding/mark-256.png"
install_size 512 "$ROOT/branding/mark-512.png"

if command -v magick >/dev/null 2>&1; then
    mkdir -p "$HICOLOR/48x48/apps"
    magick "$ROOT/branding/mark-64.png" -resize 48x48 \
        -alpha on -define png:color-type=6 \
        "$HICOLOR/48x48/apps/${APP_ID}.png"
    ln -sfn "${APP_ID}.png" "$HICOLOR/48x48/apps/tofv-app.png"
fi

# TryExec = path only (spec). Exec may take --tray for autostart.
APP_BIN="$BINDIR/tofv-app"
sed -e "s|@TRYEXEC@|$APP_BIN|" -e "s|@EXEC@|$APP_BIN|" \
    "$ROOT/packaging/linux/dev.tofv.desktop" > "$APPS/${APP_ID}.desktop"
# Plasma matche le basename du .desktop sur l’app_id Wayland.
sed "s|^StartupWMClass=.*|StartupWMClass=tofv-app|" "$APPS/${APP_ID}.desktop" \
    > "$APPS/tofv-app.desktop"
chmod 644 "$APPS/${APP_ID}.desktop" "$APPS/tofv-app.desktop"

AUTOSTART="${XDG_CONFIG_HOME:-$HOME/.config}/autostart"
mkdir -p "$AUTOSTART"
{
    sed -e "s|@TRYEXEC@|$APP_BIN|" -e "s|@EXEC@|$APP_BIN --tray|" \
        "$ROOT/packaging/linux/dev.tofv.desktop"
    echo "Hidden=false"
    echo "X-GNOME-Autostart-enabled=true"
} > "$AUTOSTART/${APP_ID}.desktop"
chmod 644 "$AUTOSTART/${APP_ID}.desktop"

if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -f -t "$HICOLOR" 2>/dev/null || true
fi
if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$APPS" 2>/dev/null || true
fi
if command -v kbuildsycoca6 >/dev/null 2>&1; then
    kbuildsycoca6 >/dev/null 2>&1 || true
elif command -v kbuildsycoca5 >/dev/null 2>&1; then
    kbuildsycoca5 >/dev/null 2>&1 || true
fi

echo "OK. Icône thème : $HICOLOR (name $APP_ID)"
echo "    Lanceur     : $APPS/${APP_ID}.desktop  (double-clic, pas de terminal)"
echo "    Autostart   : $AUTOSTART/${APP_ID}.desktop  (tray au login, pas d’autoconnect)"
echo "    Binaire     : $BINDIR/tofv-app -> $BIN"
echo
echo "Lance depuis le menu Applications, ou : tofv-app"
echo "Le terminal n’a pas besoin de rester ouvert."
echo "Systray : sudo pacman -S libayatana-appindicator  (sinon le panneau s’ouvre)"
