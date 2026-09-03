# Arch-based toolchain so the Tauri binary links like the CachyOS/Arch host.
FROM docker.io/library/archlinux:base-devel

# Pin a font provider so gtk3 does not ask interactively.
RUN pacman -Syu --noconfirm --needed --disable-download-timeout ttf-dejavu \
    && pacman -S --noconfirm --needed --disable-download-timeout \
        rust \
        gtk3 \
        webkit2gtk-4.1 \
        libayatana-appindicator \
        librsvg \
        clang \
    && pacman -Scc --noconfirm

WORKDIR /src
