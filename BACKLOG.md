# Backlog TOFV

Priorités :

| Tag | Sens |
| --- | --- |
| **P0** | Douleur du quotidien, à faire ensuite |
| **P1** | Gros gain UX / sécu, une fois P0 calé |
| **P2** | Produit « vrai client VPN » (profils, paquet) |
| **P3** | Plus tard / autre plateforme |
| **Won't** | Hors scope volontaire |

Effort : S (heures), M (1–3 jours), L (semaine+).

---

## Déjà en place (ne pas replanifier)

- Wrapper `openfortivpn` (pas de réimplémentation du protocole)
- Profil unique disque + mot de passe via **Secret Service** (`secret-tool` / libsecret : KWallet, gnome-keyring, …)
- TOTP 6 chiffres **saisi** (F121 / FortiToken), config éphémère `0600`, jamais de secret en argv
- Pinning `trusted-cert` + parse d’erreur openfortivpn
- Helper de commande redactée + journal (10 lignes dans le panneau, fenêtre dédiée)
- `tofv-helper` + Polkit `allow_active=yes` (Connect/Couper sans sudo, **après** `install-helper.sh`)
- UI Tauri : chrome custom, header tofu + LED figés, taille mini 720×560
- Reprise d’un tunnel orphelin (`ppp0` / pid helper) après kill de l’UI
- Élévation : UI jamais root
- Build Podman (Arch) + `./scripts/build-app.sh`
- Logo / icônes RGBA + `.desktop` / hicolor via `scripts/install-desktop.sh`
- Rotation `trusted-cert` : retry sans pin, comparer SHA, épingler et reconnecter (nouveau TOTP)
- Doctor (CLI `tofv doctor` + écran bloquant) + `scripts/install.sh` (release, helper, PATH)
- Trousseau **Secret Service** (agnostique KDE/GNOME), pas KWallet en dur
- Instance unique (socket) + détache du TTY (`tofv-app` ne garde pas le terminal)
- Autostart XDG `--tray` (install-desktop + `/etc/xdg/autostart` du paquet)
- `.desktop` `Terminal=false` : double-clic sans terminal
- Connect tray / panneau → fenêtre TOTP (pas le panneau) ; auth fail → nouveau code tout de suite
- Audit sécu H1–H3 : conf recopié root (anti-TOCTOU), plus de `pkexec /bin/sh`, pas d’`openfortivpn` brut en pkexec

---

## P0 — boucler le flux quotidien

| ID | Item | Effort | Pourquoi |
| --- | --- | --- | --- |
| P0-1 | **Tray StatusNotifier fiable** : `libayatana-appindicator`, icône connecté/déconnecté (tooltip déjà là), menu Connect / Couper / Ouvrir / Quitter, clic gauche = panneau | M | Sans la lib le panneau s’ouvre tout seul ; le contrat « daemon barre de tâches » n’est pas tenu |
| P0-2 | **Autostart XDG** — tray seulement, **pas** d’autoconnect | S | Fait (`install-desktop.sh` + PKGBUILD `/etc/xdg/autostart`) |
| P0-3 | **First-run / doctor bloquant** | M | Fait (`tofv doctor` + écran UI) |
| P0-4 | **Connect depuis le tray** → popup TOTP + toasts si pas de mot de passe / pas de helper | S | Fait (fenêtre TOTP, pas le panneau ; toast mdp / prérequis) |
| P0-5 | **Popup TOTP** : focus, collage, bouton hors 6 chiffres (déjà là) — peaufiner messages d’erreur F121 | S | Geste quotidien |
| P0-6 | **Retry TOTP** : si auth fail, rouvrir la popup tout de suite | M | Fait (message F121 60 s, champ vidé, focus) |
| P0-8 | **Journal** : « copier la commande redactée », niveaux (fenêtre + borne 400 déjà là) | S | Debug |
| P0-9 | **État Couper** : messages si helper absent / pkexec | S | En partie fait |
| P0-10 | **Lanceur** : `tofv-app` dans `~/.local/bin` + `.desktop` (script `install-desktop.sh` déjà là) | S | Fait ; double-clic / `tofv-app` détache du TTY |

---

## P1 — UX

### UI / UX panneau

| ID | Item | Effort |
| --- | --- | --- |
| P1-U1 | Distinguer **Réglages** (profil) et **Session** (connect / logs) | M |
| P1-U2 | Masquer host/port/realm derrière « Avancé » une fois le profil OK | M |
| P1-U3 | Notifications desktop (connecté, drop, cert changé) | S |
| P1-U4 | Polices **locales** (plus Google Fonts) | S |
| P1-U5 | Accessibilité : focus trap des modales (Escape déjà là), contraste LED | S |
| P1-U6 | Confirmer l’enregistrement mot de passe (entrée `dev.tofv` dans le trousseau) | S |
| P1-U7 | Afficher l’IP `ppp0` / passerelle une fois UP | S |

### Prérequis & installation runtime

| ID | Item | Effort |
| --- | --- | --- |
| P1-D1 | Détecter `openfortivpn` / `pppd` / `pkexec` / helper / `libayatana` + commande distro | M |
| P1-D2 | Bouton « Installer le helper » (`pkexec` sur `install-helper.sh`) | M |
| P1-D3 | Si helper absent : Connect **refuse** (doctor bloquant) — plus de pkexec openfortivpn | S | Fait (H3) |
| P1-D4 | `tofv doctor` CLI = même rapport, exit ≠ 0 si bloquant | S |

---

## P2 — profils, paquet, livraison

### Multi-profils

| ID | Item | Effort |
| --- | --- | --- |
| P2-P1 | Liste de profils, profil actif dans le tray | M |
| P2-P2 | Mot de passe **par** profil (déjà le modèle keyring = id) | S |
| P2-P3 | Dupliquer / renommer / supprimer (wipe keyring) | S |
| P2-P4 | Import `openfortivpn` config (ignorer `password =`) | S |
| P2-P5 | Export config **sans** secrets | S |

### Packaging & livraison

| ID | Item | Effort |
| --- | --- | --- |
| P2-L1 | **PKGBUILD / AUR** (CachyOS/Arch), policy Polkit avec le vrai prefix | L |
| P2-L2 | `.deb` (Debian/Ubuntu 24.04+) | L |
| P2-L3 | Template Polkit (`@LIBEXEC@`) | S |
| P2-L4 | Builds **release** (`--release`), strip | S |
| P2-L5 | CI GitHub Actions **avec le Containerfile** + artefact `tofv-VERSION-linux-x64.tar.gz` (`install-bin.sh`, README chemin 3) | M |
| P2-L6 | Tags `vX.Y.Z`, changelog, checksums | S |
| P2-L7 | **Pas de Flatpak unique** pour le helper (pppd) | — |
| P2-L8 | Mise à jour du helper : réinstall policy si le path change | M |

### Observabilité

| ID | Item | Effort |
| --- | --- | --- |
| P2-O1 | Logs persistants optionnels `~/.local/share/tofv/session.log` | S |
| P2-O2 | `--persistent` openfortivpn + backoff visible | M |
| P2-O3 | Health : `ppp0` disparaît → Error + notif | M |

---

## P3 — plus loin

| ID | Item | Effort |
| --- | --- | --- |
| P3-1 | SAML / `--saml-login` ou cookie `openfortivpn-webview` | L |
| P3-2 | Certificat client PEM / PKCS#11 (YubiKey) | L |
| P3-3 | macOS : helper SMJobBless / LaunchDaemon, Keychain | L |
| P3-4 | Options réseau avancées (no-routes, half-internet-routes) | M |
| P3-5 | i18n EN | S |
| P3-6 | Thème clair / `color-scheme` Plasma | S |

---

## Sécu / audit

| ID | Item | Sévérité | Notes |
| --- | --- | --- | --- |
| S-1 | **Revue helper** : whitelist, ownership 0600, pid = `openfortivpn`, pas de shell interpolé | haute | Fait : plus de `pkexec /bin/sh -c` ; élévation helper-only |
| S-2 | Policy Polkit `allow_active=yes` : documenter ; option `auth_admin_keep` | moyenne | OK VPN perso |
| S-3 | Socket pinentry : `0600`, unlink, pas de password en argv | haute | Déjà le design |
| S-4 | Redact : OTP, password, `SVPNCOOKIE` dans logs **et** « copier » | haute | Corpus de logs réels |
| S-6 | Wrapper pinentry **root** (`/run/tofv/UID/`) : pas un script user-writable | haute | Fait (`0700` root + `session.conf` root `0600`) |
| S-7 | `install-helper.sh` : `install -m 755` root:root, dire si c’est un debug | moyenne | P2-L4 |
| S-8 | Capabilities Tauri : revue à chaque nouvelle commande | moyenne | |
| S-9 | Pas de `insecure-ssl` dans l’UI, jamais | haute | Déjà banni du helper |
| S-10 | Tests d’attaque : `pppd-plugin`, path `../../etc/shadow`, pid forgé | haute | Fait (symlink hors runtime, plugin, valeurs, mode 0644) |
| S-11 | SBOM + `cargo audit` en CI | basse | P2-L5 |
| S-12 | Session leftover : stop au crash (reprise UI déjà là) | moyenne | |

---

## Won't (rappel)

- Windows
- Réimplémenter Fortinet / remplacer NetworkManager
- Stocker password/OTP dans `~/.config`
- GUI en root / setuid `tofv-app`
- `NOPASSWD: /usr/bin/openfortivpn`
- Autoconnect sans TOTP
- **Générateur TOTP / seed OATH / `totp-auto` / `totp-show` / import QR** — FortiToken F121 et FTM : le QR est un code d’activation, pas un secret OATH. Saisie manuelle uniquement.

---

## Ordre suggéré (prochaines itérations)

1. **P0-1** — icône tray connecté/déconnecté (autostart + lanceur déjà là)  
2. **P1-D2** — bouton UI « Installer le helper »  
3. **S-2** — documenter `allow_active=yes` / option `auth_admin_keep`  
5. **P2-L1 + P2-L3 + P2-L4 + P2-L5** — AUR + CI  
6. **P2-P1** — multi-profils si 2ᵉ VPN  

Ce fichier est le contrat de suite. On coche ici, pas dans le README.
