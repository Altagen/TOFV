# TOFV backlog

Priorities:

| Tag | Meaning |
| --- | --- |
| **P0** | Daily friction, do next |
| **P1** | Large UX / security win, once P0 is settled |
| **P2** | "Real VPN client" territory (profiles, packaging) |
| **P3** | Later, or another platform |
| **Won't** | Deliberately out of scope |

Effort: S (hours), M (1–3 days), L (a week or more).

---

## Already in place (do not re-plan)

- Wraps `openfortivpn`; the protocol is not reimplemented
- Single on-disk profile, password in **Secret Service** (`secret-tool` / libsecret: KWallet, gnome-keyring, …)
- 6-digit code **typed by hand**, ephemeral `0600` config, no secret ever in argv
- `trusted-cert` pinning plus openfortivpn error parsing
- Redacted command view and log (tail in the panel, dedicated window)
- `tofv-helper` + Polkit `allow_active=yes` (Connect/Disconnect without sudo, **after** `install-helper.sh`)
- Tauri UI: custom chrome, fixed header and LED, 720×560 minimum size
- Adopts an orphaned tunnel (`ppp0` / helper pid) after the UI is killed
- Elevation: the UI is never root
- Podman build (Arch) plus `./scripts/build-app.sh`
- Logo / RGBA icons, `.desktop` and hicolor via `scripts/install-desktop.sh`
- `trusted-cert` rotation: retry unpinned, compare the SHA, pin and reconnect
- Doctor (`tofv doctor` CLI plus a blocking first-run screen) and `scripts/install.sh`
- Single instance (socket) and TTY detach — `tofv-app` does not hold the terminal
- XDG autostart with `--tray`, `.desktop` with `Terminal=false`
- Connect from tray or panel opens the code window; auth failure asks for a fresh code immediately
- Constant-time log ingestion: `openfortivpn -v` logs one line per packet, and
  a quadratic parser used to stall the pipe and freeze the tunnel
- Helper reads and validates the config on one file descriptor (`O_NOFOLLOW`
  plus `fstat`), so the path cannot be swapped between check and read
- Rejection messages never echo file content, only line numbers
- Pinentry socket checks the peer uid (`SO_PEERCRED`) and caps how many times
  it will hand out the password

---

## P0 — close the daily loop

| ID | Item | Effort | Why |
| --- | --- | --- | --- |
| P0-1 | StatusNotifier tray — the state icon is done in 0.1.1 (grey / grey with an amber dot / colour). What remains is the `libayatana-appindicator` dependency itself: without it there is no tray at all and the panel opens instead | M | |
| P0-5 | **Code prompt**: focus, paste, polish the error messages | S | The everyday gesture |
| P0-8 | **Log**: "copy the redacted command", log levels | S | Debugging |
| P0-9 | **Disconnect state**: clearer messages when the helper or pkexec is missing | S | Partly done |

---

## P1 — UX

### Panel

| ID | Item | Effort |
| --- | --- | --- |
| P1-U1 | Separate **Settings** (profile) from **Session** (connect / logs) | M |
| P1-U2 | Hide host/port/realm behind "Advanced" once the profile is valid | M |
| P1-U3 | Desktop notifications (connected, dropped, certificate changed) | S |
| P1-U4 | ~~Bundle the fonts locally~~ — done in 0.1.1: IBM Plex Mono and Oxanium ship as woff2 (44 KB, OFL 1.1), and the Tauri CSP no longer allows fonts.googleapis.com or fonts.gstatic.com at all | S |
| P1-U5 | Accessibility: focus trap in modals, LED contrast | S |
| P1-U6 | Confirm the password was stored (show the `dev.tofv` keyring entry) | S |
| P1-U7 | Show the `ppp0` address and gateway once up | S |

### Prerequisites and runtime install

| ID | Item | Effort |
| --- | --- | --- |
| P1-D1 | Detect `openfortivpn` / `pppd` / `pkexec` / helper / `libayatana` and print the distro command | M |
| P1-D2 | An "Install the helper" button (`pkexec` on `install-helper.sh`) | M |
| P1-D4 | `tofv doctor` prints the same report, non-zero exit when blocking | S |

---

## P2 — profiles, packaging, delivery

### Multiple profiles

| ID | Item | Effort |
| --- | --- | --- |
| P2-P1 | Profile list, active profile shown in the tray | M |
| P2-P2 | Password **per** profile (the keyring model is already keyed by id) | S |
| P2-P3 | Duplicate / rename / delete (wiping the keyring entry) | S |
| P2-P4 | Import an `openfortivpn` config (ignoring `password =`) | S |
| P2-P5 | Export the config **without** secrets | S |

### Packaging and delivery

| ID | Item | Effort |
| --- | --- | --- |
| P2-L1 | **PKGBUILD / AUR** (Arch, CachyOS), Polkit policy with the real prefix | L |
| P2-L2 | `.deb` (Debian / Ubuntu 24.04+) | L |
| P2-L4 | Release builds (`--release`), stripped | S |
| P2-L5 | ~~Release artifact, SBOM and checksums~~ — done: `release.yml` publishes `tofv-{version}-linux-x86_64.tar.gz`, a CycloneDX SBOM and `SHA256SUMS.txt` | M |
| P2-L6 | Changelog generated from the Conventional Commits history (cliff.toml, as in Ora/Rite) | S |
| P2-L7 | **No single Flatpak** for the helper (pppd) | — |
| P2-L8 | Helper upgrades: reinstall the policy when the path changes | M |
| P2-L9 | `linux-aarch64` release. Nothing technical blocks it — Debian builds `openfortivpn` and `libwebkit2gtk-4.1` for arm64, and aarch64 is a Tier 1 Rust target. What is missing is an ARM container base (`archlinux:base-devel` is x86_64 only) and someone who can actually run the result. Skip armhf/armel | M |

### Observability

| ID | Item | Effort |
| --- | --- | --- |
| P2-O1 | Optional persistent log at `~/.local/share/tofv/session.log` | S |
| P2-O2 | openfortivpn `--persistent` with visible backoff | M |
| P2-O3 | Health: `ppp0` disappears → Error plus a notification | M |

---

## P3 — further out

| ID | Item | Effort |
| --- | --- | --- |
| P3-1 | SAML via `--saml-login`, or an `openfortivpn-webview` cookie | L |
| P3-2 | Client certificate, PEM or PKCS#11 (YubiKey) | L |
| P3-3 | macOS: SMJobBless / LaunchDaemon helper, Keychain | L |
| P3-4 | Advanced network options (no-routes, half-internet-routes) | M |
| P3-6 | Light theme / Plasma `color-scheme` | S |

---

## Security / audit

| ID | Item | Severity | Notes |
| --- | --- | --- | --- |
| S-1 | **Helper review**: allowlist, `0600` ownership, pid is `openfortivpn`, no interpolated shell | high | Done — no `pkexec /bin/sh -c`, helper-only elevation |
| S-2 | Document the `allow_active=yes` Polkit policy; offer `auth_admin_keep` | medium | Fine for a personal VPN, needs saying out loud |
| S-3 | Pinentry socket: `0600`, unlinked, no password in argv | high | By design |
| S-4 | Redact the OTP, password and `SVPNCOOKIE` in the log **and** in "copy" | high | Needs a real-world log corpus to validate |
| S-6 | The **root** pinentry wrapper (`/run/tofv/UID/`) must not be a user-writable script | high | Done (`0700` root, `session.conf` root `0600`) |
| S-7 | `install-helper.sh`: `install -m 755` root:root, and say when it is a debug build | medium | See P2-L4 |
| S-8 | Tauri capabilities: review on every new command | medium | |
| S-9 | Never expose `insecure-ssl` in the UI | high | Already banned in the helper |
| S-10 | Attack tests: `pppd-plugin`, `../../etc/shadow` paths, forged pid, post-check symlink swap | high | Done |
| S-11 | ~~SBOM + `cargo audit` in CI~~ — done: `cargo deny` in CI, CycloneDX SBOM published per release | low | |
| S-14 | `glib` 0.18 `VariantStrIter` unsoundness (RUSTSEC-2024-0429) | low | Pinned by Tauri; TOFV never calls it. Clears when Tauri moves off GTK3 |
| S-15 | 16 unmaintained crates, all archived gtk-rs GTK3 bindings via Tauri | low | Listed with reasons in `deny.toml`; nothing to do until Tauri targets GTK4 |
| S-16 | ~~The published SBOM omits npm devDependencies~~ — fixed in 0.1.1 via `.syft.yaml` (`javascript.include-dev-dependencies`) | medium | Verified on the 0.1.0 release: 457 cargo crates and `@tauri-apps/api`, but **not** `typescript` or `vite` — even though vite produces the bundle that ships inside the binary. A compromised build tool is exactly what an SBOM should let you trace. syft catalogues runtime deps only for npm; needs `--select-catalogers` tuning or a separate `cyclonedx-npm` pass. Note esbuild *is* present, caught by the Go binary cataloger — so coverage is currently inconsistent rather than merely narrow |
| S-13 | The pinentry socket still serves any process running as the **same user** during a connect attempt. Inherent to the design; revisit if the threat model tightens | medium | Mitigated by peer-uid check, request cap and a short window |

---

## Won't

- Windows
- Reimplementing Fortinet, or replacing NetworkManager
- Storing the password or OTP in `~/.config`
- A root GUI, or a setuid `tofv-app`
- `NOPASSWD: /usr/bin/openfortivpn`
- Autoconnect without a one-time code
- **A built-in code generator / OATH seed / QR import** — the Fortinet QR is
  an activation payload, not an OATH secret. Manual entry only.

---

## Suggested order

1. **P1-D2** — "Install the helper" button
2. **S-2** — document `allow_active=yes` and the `auth_admin_keep` option
3. **P2-L1** — AUR package (release artifacts are done)
4. **P2-P1** — multiple profiles, once there is a second VPN to connect to

This file is the plan of record. Tick items here, not in the README.
