# TOFV — Tray OpenFortiVPN

Petit client graphique Unix pour [openfortivpn](https://github.com/adrienverge/openfortivpn).
TOFV ne réimplémente pas le protocole Fortinet : il enveloppe le binaire
`openfortivpn`, qui fait déjà le travail, et ajoute ce qui manque au quotidien :

- un daemon en barre de tâches, avec le statut (connecté / déconnecté / en cours)
  et un bouton pour ouvrir l’UI ;
- un petit panneau d’administration : un profil (hôte, port, realm, username,
  mot de passe, certificat de confiance) et le mode d’auth (TOTP pour la v1) ;
- un stockage correct des identifiants (Secret Service / trousseau du bureau, jamais en clair) ;
- une popup TOTP (6 chiffres, FortiToken, période 30 s) au moment du Connect ;
- un *helper* qui montre les commandes réellement exécutées, les journaux, et
  ce qui a été masqué (mot de passe, TOTP).

**Cibles :** Linux en premier, macOS ensuite, Unix-like si le terrain le permet.
**Hors périmètre :** Windows, réimplémentation du tunnel, FortiClient officiel.

---

## Pourquoi ce projet existe

`openfortivpn` en CLI fonctionne très bien. Ce qui est pénible, ce n’est pas le
VPN, c’est la *cérémonie* à chaque session :

1. se souvenir du `trusted-cert` (empreinte SHA-256 du certificat passerelle) ;
2. fournir `username`, `password`, `realm` ;
3. lire le TOTP à 6 chiffres dans FortiToken (le code tourne toutes les 30 s) ;
4. lancer le tout en `sudo`, puis surveiller le process.

Les alternatives existantes ne couvrent pas ce besoin proprement :

| Outil | Limite pour ce cahier des charges |
| --- | --- |
| `openfortivpn` seul | Aucune mémoire de session, aucun tray, mot de passe trop souvent passé en clair (`-p` visible dans `ps`) |
| NetworkManager-fortisslvpn | Wrapper officiel, mais OTP/SAML irréguliers, peu de visibilité sur la commande réellement lancée |
| FortiClient propriétaire | Lourd, moins bon citoyen Linux, pas un wrapper du binaire libre |
| [openfortivpn-webview](https://github.com/gm-vm/openfortivpn-webview) | Utile uniquement pour récupérer un cookie SAML |

TOFV se place **au-dessus** d’`openfortivpn`, pas à côté.

---

## Ce que TOFV n’est pas

- Pas un nouveau client VPN. Le tunnel, TLS, PPP, routes et DNS restent
  entièrement du ressort d’`openfortivpn` + `pppd`.
- Pas un store de secrets maison. Linux : **Secret Service** (`secret-tool` /
  libsecret) — KWallet, gnome-keyring ou tout autre daemon compatible.
  macOS plus tard : Keychain, même trait `PasswordStore`.
- Pas une app Windows. Aucun effort de portage n’est prévu.
- Pas un remplacement de NetworkManager. On peut cohabiter, on ne s’y greffe pas.

---

## Ce qu’il faut savoir sur openfortivpn

Sources : [README officiel](https://github.com/adrienverge/openfortivpn/blob/master/README.md)
et [man openfortivpn(1)](https://manpages.debian.org/testing/openfortivpn/openfortivpn.1.en.html).

### Rôle du binaire

`openfortivpn` établit un tunnel **PPP+TLS** vers une passerelle Fortinet.
Il spawn `pppd`, puis configure routes et DNS. Il est packagé partout
(Debian/Ubuntu, Fedora, Arch, NixOS, Homebrew, MacPorts…).

### Paramètres qui comptent pour TOFV

| Besoin utilisateur | Option CLI | Clé de config | Remarque |
| --- | --- | --- | --- |
| Passerelle | `host[:port]` | `host`, `port` | Port fréquent : `443` ou `8443` |
| Identifiant | `-u` / `--username` | `username` | |
| Mot de passe | `-p` / `--password` | `password` | **Ne jamais passer en argv** (lisible dans `ps`). Préférer `pinentry` ou un fichier de config `0600` temporaire |
| Realm | `--realm` | `realm` | Souvent vide ; obligatoire sur certaines fermes |
| OTP | `-o` / `--otp` | `otp` | La man page dit explicitement que `otp =` est « useful for a gui » |
| Invite OTP | `--otp-prompt` | `otp-prompt` | Si le serveur n’utilise pas le prompt par défaut |
| Délai OTP | `--otp-delay` | `otp-delay` | Utile si le token n’est pas encore valide |
| Désactiver le push FTM | `--no-ftm-push` | `no-ftm-push` | Force l’OTP à la place d’une notif FortiToken Mobile |
| Certificat passerelle | `--trusted-cert` | `trusted-cert` | Empreinte **SHA-256** du certificat X.509 en DER. Répétable |
| Mot de passe via pinentry | `--pinentry=` | `pinentry` | Voie officielle « sécurisée » |
| Fichier de config | `-c` / `--config` | — | Destiné aux GUI. CLI override le fichier |
| Cookie SAML | `--cookie` / `--cookie-on-stdin` | — | Pour SSO via navigateur / webview |
| SSO natif | `--saml-login` | — | Mini serveur local (défaut `:8020`) qui récupère la redirection SAML |
| Certificat client | `--user-cert` / `--user-key` | `user-cert`, `user-key` | PEM ou `pkcs11:` (YubiKey, etc.) |
| Reconnexion | `--persistent=` | `persistent` | Boucle infinie, intervalle en secondes |
| Verbosité | `-v` / `-q` | | TOFV lancera au moins `-v` pour alimenter le helper |

Options volontairement **non exposées** dans l’UI v1, ou derrière un mode
avancé verrouillé : `--pppd-plugin`, `--pppd-log`, `--insecure-ssl`.
Le README d’openfortivpn le dit noir sur blanc : un utilisateur malveillant
qui peut passer `--pppd-plugin` à un `openfortivpn` root possède une
exécution de code. TOFV ne doit jamais relayer ces flags depuis l’UI.

### Certificat de confiance (`trusted-cert`)

Beaucoup de passerelles Fortinet ont un certificat que la PKI système ne
valide pas (auto-signé, chaîne incomplète, nom qui ne matche pas).
`openfortivpn` refuse alors la connexion et imprime :

```
ERROR: Gateway certificate validation failed, and the certificate digest is not in the local whitelist.
If you trust it, rerun with:
    --trusted-cert <sha256>
```

C’est **le** flux de premier lancement. TOFV doit :

1. tenter une connexion sans `trusted-cert` (ou avec celui déjà enregistré) ;
2. parser cette erreur ;
3. afficher une fenêtre « Faire confiance à cette passerelle ? » avec
   l’empreinte, le host et le port ;
4. enregistrer l’empreinte dans le profil, puis relancer.

On peut aussi pré-calculer l’empreinte :

```sh
echo | openssl s_client -connect vpn.example.com:443 2>/dev/null \
  | openssl x509 -outform DER | sha256sum
```

`--insecure-ssl` n’est **pas** une alternative acceptable.

### Mot de passe : trois voies, une seule correcte pour une GUI

1. **`-p motdepasse`** — interdit. Visible dans `/proc/<pid>/cmdline`.
2. **`password =` dans un fichier durable** — interdit. Fichier qui survit,
   backupé, lisible par un copier-coller malheureux.
3. **`pinentry` ou config éphémère `0600`** — c’est la voie TOFV.

La man page recommande pinentry (`pinentry-gnome3`, `pinentry-qt`,
`pinentry-mac`). Pour une GUI qui *détient déjà* le secret dans le trousseau,
le plus propre est un **pinentry TOFV** : petit binaire qui parle le
protocole pinentry et renvoie le mot de passe lu dans le keyring, sans
dialogue supplémentaire. L’OTP, lui, est écrit dans un fichier de config
temporaire au moment du clic « Connecter » (c’est le contrat officiel GUI),
puis le fichier est détruit.

### Privilèges : openfortivpn doit être root, l’UI ne doit pas l’être

`openfortivpn` a besoin des droits root à trois moments :

- spawn de `/usr/sbin/pppd` ;
- pose des routes une fois le tunnel UP ;
- écriture des nameservers (`/etc/resolv.conf` ou `resolvconf`).

Conséquence d’architecture : **le process tray tourne en user**. Seul
l’enfant `openfortivpn` est élevé. Trois mécanismes possibles, du plus
correct au plus rustique :

| Mécanisme | Plateforme | Commentaire |
| --- | --- | --- |
| Polkit (`pkexec` + règle `.policy`) | Linux | Meilleur UX : dialogue graphique, scopeable. À privilégier |
| sudoers dédié, commande **figée** | Linux / macOS | Acceptable si la ligne de commande est un allowlist sans wildcards dangereux |
| Saisie du mot de passe sudo à chaque fois | partout | Filet de sécurité, UX médiocre |

Un `ALL=(ALL) NOPASSWD: /usr/bin/openfortivpn` ouvert est une faille
(`--pppd-plugin`). Le sudoers / polkit de TOFV doit n’autoriser que
`openfortivpn -c <fichier sous /run/user/$UID/tofv/…>` (ou équivalent),
sans flags libres.

### TOTP (le seul second facteur de la v1)

La passerelle de référence demande un **TOTP FortiToken** à 6 chiffres,
période 30 s, lu dans l’appli après scan du QR code. Ce n’est pas un push,
pas du SAML, pas un code mail.

Séquence réelle côté `openfortivpn` : username + password, **puis** le
serveur réclame le token. Le TOTP doit donc être fourni **avant** (ou au
moment où) openfortivpn authentifie — pas « après que le binaire a réussi ».
Si on attend que le tunnel soit UP, c’est trop tard.

TOFV le fait ainsi :

1. clic Connect (tray ou panneau) ;
2. popup « Code TOTP », champ à 6 chiffres lus sur le FortiToken ;
3. écriture de `otp = <code>` dans la config temporaire `0600` ;
4. élévation d’`openfortivpn -c … --pinentry=pinentry-tofv` ;
5. destruction de la config temporaire.

`--no-ftm-push` sera passé par défaut : on ne veut pas qu’un push mobile
court-circuite le champ TOTP.

Pas de générateur TOTP : le QR FortiToken / F121 n’est **pas** un seed OATH
(`otpauth://`). Le secret reste dans le jeton ; TOFV ne fait que le 6 chiffres
saisi. SAML / cookie / `--saml-login` restent hors v1.

### macOS

`openfortivpn` est dans Homebrew et MacPorts. Sur les macOS récents, le
`pppd` Apple est ancien : le README officiel recommande
`--enable-legacy-pppd` si on compile soi-même. Le wrapper TOFV n’a rien à
changer au protocole, mais le packaging macOS devra :

- vérifier la présence du binaire (`which openfortivpn`) ;
- vivre en `LSUIElement` / `ActivationPolicy::Accessory` (pas d’icône Dock) ;
- utiliser le Keychain ;
- élever via sudo (pas de Polkit).

---

## Choix de stack : Go ou Rust, Tauri ou pas

Le critère du cahier des charges n’est pas « le plus hype », c’est
**sécurité des secrets + petit daemon tray Unix + UI qui explique ce qu’elle
fait**. La langue pèse moins que l’architecture (pas de mot de passe en
argv, UI non-root, allowlist d’élévation). Une fois ça posé, il reste à
choisir l’outil qui rend cette architecture naturelle.

### Pondération

| Critère | Poids | Rust | Go |
| --- | --- | --- | --- |
| Hygiène mémoire autour des secrets (zeroize, pas de copies fantômes faciles) | élevé | mieux | correct, GC qui retient les strings plus longtemps |
| Modèle de capabilities pour une UI desktop | élevé | Tauri 2 a un modèle ACL natif | Wails / Fyne : plus permissif par défaut |
| Tray Linux (StatusNotifier / AppIndicator) + macOS (`NSStatusItem`) | élevé | Tauri 2, `tray-icon` | `fyne.io/systray`, Fyne |
| Fenêtre « helper » (logs, commande, profils) | élevé | webview système, frontend TypeScript | Fyne (natif, plus pauvre) ou Wails (webview) |
| Intégration keyring (libsecret / Keychain) | élevé | crate `keyring` mature | `zalando/go-keyring` mature aussi |
| Supervision de process, parse de stderr, fichiers `0600` | moyen | excellent | excellent, souvent plus court |
| Poids du « petit daemon » | moyen | Tauri tire WebKitGTK sur Linux | Fyne / systray plus léger ; Wails comparable à Tauri |
| Chaîne de build | moyen | rustc + (si Tauri) Node | un seul `go build` |
| Précedent industrie (client VPN desktop) | faible | Firezone a choisi Tauri pour Linux | beaucoup de trays Go (syncthing, etc.) |

Go n’est **pas** un mauvais choix. Un daemon `systray` + une petite fenêtre
Fyne irait plus vite à un MVP rustique, avec un binaire unique et sans
WebKit. Ce qui le fait perdre ici :

- le cahier des charges demande un **front** (helper, détail des commandes,
  formulaires de profils) et un **back**. Fyne force tout en Go et produit
  une UI moins agréable pour ce type d’outil ;
- Wails (Go + webview) recopie Tauri sans le modèle de permissions ni
  l’écosystème tray/keyring aussi cadré ;
- pour un programme qui touche sudo, mot de passe et OTP, le coût d’un
  frontend web local *sableboxed* (CSP, IPC allowlist) vaut le surcoût.

### Décision

**Rust + Tauri 2 + TypeScript**, tray-first.

Pourquoi ce trio, concrètement :

- **Rust** porte le cœur : profils, trousseau, génération de la config
  temporaire, spawn/supervision d’`openfortivpn`, parse des erreurs
  `trusted-cert`, journalisation *redactée*. Moins de surprises autour des
  secrets, et un seul langage pour le code privilégié-adjacent.
- **Tauri 2** est le cadre desktop qui va avec : tray officiel
  Linux/macOS, fenêtres à la demande, IPC typé, ACL (le frontend n’a *pas*
  le droit d’écrire un sudoers ni de lancer un argv arbitraire). Firezone
  s’en sert déjà pour un client VPN Linux. Sur macOS on masque le Dock
  (`ActivationPolicy::Accessory`).
- **TypeScript** (frontend web local, pas un serveur distant) pour le
  panneau d’admin, la popup TOTP, le dialogue « faire confiance au
  certificat », et le helper de commandes. C’est le bon outil pour une UI
  de formulaires + logs. Framework UI volontairement mince
  (pas besoin d’un SPA d’entreprise).

Ce que Tauri n’est **pas**, dans ce projet :

- pas une appli Electron ;
- pas une fenêtre permanente. Le process démarre **en tray seulement**.
  le panneau et les popups (TOTP, certificat) s’ouvrent à la demande et
  se ferment sans tuer le daemon ;
- pas une excuse pour faire tourner le webview en root.

### Alternatives explicitement écartées

| Option | Pourquoi non (pour *ce* projet) |
| --- | --- |
| Go + Fyne | Plus léger, mais UI trop pauvre pour le helper / les flux certificat+OTP |
| Go + Wails | Même forme que Tauri, moins de garanties IPC / ACL |
| Rust + iced / egui | UI 100 % Rust, tray et accessibilité plus faibles, plus long à peaufiner |
| Rust natif `tray-icon` sans fenêtre | Trop petit : on perd le helper demandé |
| Electron | Hors-sujet (poids, surface, Windows-first) |
| Greffe NetworkManager | On perd le contrôle de la commande et l’OTP est déjà un point faible |

Si WebKitGTK devient un vrai problème sur une distro minimale, le cœur
`tofv-core` reste réutilisable derrière un autre shell (iced, ou même un
tray Go). C’est pour ça que le cœur n’est **pas** collé à Tauri.

---

## Architecture

```
 ┌─────────────────────────────────────────────┐
 │  tofv (session utilisateur, non-root)       │
 │                                             │
 │  tray ──► statut + Connect / Disconnect     │
 │    │      + « Ouvrir l'UI »                 │
 │    │                                        │
 │    └─ panneau d'admin (une fenêtre)         │
 │         profil unique, TOTP, helper, cert   │
 │                                             │
 │  tofv-core ── keyring (libsecret / Keychain)│
 │       │                                     │
 │       ├─ écrit /run/user/$UID/tofv/<id>.conf│
 │       │   (0600, otp inclus, password non)  │
 │       └─ pinentry-tofv (protocole pinentry) │
 └───────────────┬─────────────────────────────┘
                 │  pkexec / sudo allowlisté
                 ▼
         openfortivpn -c <conf> --pinentry=pinentry-tofv -v
                 │
                 ▼
               pppd + routes + DNS
```

### Découpe logicielle

```
TOFV/
├── README.md                 ← ce fichier
├── crates/
│   ├── tofv-core/            # profils, secrets, spawn, parse, redact
│   └── pinentry-tofv/        # helper pinentry qui lit le keyring
├── src-tauri/                # shell Tauri 2 : tray, fenêtres, IPC
├── ui/                       # frontend TypeScript
└── packaging/
    ├── linux/                # .desktop, icônes, polkit, sudoers.example
    └── macos/                # bundle Accessory, notes Homebrew
```

- **`tofv-core`** ne dépend pas de Tauri. Testable en CLI
  (`tofv-core connect --profile work --otp 123456 --dry-run`).
- **`pinentry-tofv`** est un binaire minuscule, invoqué par openfortivpn
  root. Il ne reçoit aucun secret en argument : il va le chercher dans le
  trousseau de l’utilisateur (via le session bus / Keychain).
- **`src-tauri`** n’est que le shell : tray, notifications, ouverture des
  fenêtres, exposition d’une API IPC *étroite*
  (`get_profile`, `save_profile`, `connect`, `disconnect`, `trust_cert`,
  `get_logs`).
- **`ui`** n’a aucun accès filesystem / process hors de cette API.

### Données persistantes

```
~/.config/tofv/config.toml          # préférences UI, profil actif
~/.config/tofv/profiles/<id>.toml   # host, port, realm, trusted-cert, user
                                    # JAMAIS le mot de passe, JAMAIS l’OTP
```

Secrets dans le trousseau, service `dev.tofv`, compte = id de profil :

- `password`

Fichiers runtime sous `/run/user/$UID/tofv/` (ou `$TMPDIR` macOS avec
`0600` + `O_EXCL`) : config openfortivpn éphémère, fifo de logs.

### Helper de commandes

Le helper n’est pas un terminal cosmétique. Il affiche :

1. **la commande réellement lancée**, déjà redactée :

   ```
   pkexec /usr/local/libexec/tofv-helper start \
     --config /run/user/1000/tofv/default.conf
   ```

2. **le contenu de la config temporaire**, avec `otp = ******` (le TOTP) ;
3. **stdout/stderr live** d’openfortivpn, avec filtre qui masque toute
   ligne ressemblant à un mot de passe, cookie `SVPNCOOKIE`, ou OTP ;
4. l’état interne TOFV : *idle / resolving cert / waiting otp / elevating /
   connecting / up / reconnecting / error*.

Un bouton « copier la commande » copie la version redactée, jamais les
secrets.

---

## Modèle de sécurité (non négociable)

1. L’UI et le tray ne tournent jamais en root.
2. Le mot de passe ne passe jamais en argument de process, ni dans
   `~/.config`, ni dans les logs, ni dans le presse-papier du helper.
3. Le TOTP ne vit que dans la config temporaire le temps de la tentative,
   puis le fichier est unlinked. Jamais dans le keyring en v1.
4. `trusted-cert` est un *pinning* explicite, pas un « accepter tout ».
5. `--insecure-ssl`, `--pppd-plugin`, `--pppd-log` sont absents de l’API IPC.
6. L’élévation est **uniquement** `tofv-helper` (Polkit). Jamais
   `pkexec openfortivpn` ni `pkexec /bin/sh`. Le helper recopie le conf
   validé dans `/run/tofv/<uid>/` (root, `0600`) avant `exec`.
7. Le frontend TypeScript n’a pas `shell` / `fs` ouverts : uniquement les
   commandes Tauri déclarées.
8. Dry-run : toute action a un mode « montrer sans exécuter ».

Ces règles pèsent plus que le choix Rust. Un wrapper Go qui les respecte
serait plus sûr qu’un wrapper Rust qui lance `sudo openfortivpn -p …`.

---

## Fonctionnalités

### MVP (v1) — état

Cible de référence : **KDE Plasma**. Linux d’abord.

La suite (TOTP auto, profils, AUR, audit helper, tray, etc.) est dans
[`BACKLOG.md`](BACKLOG.md).

- [x] Panneau : un profil, TOTP manuel, mot de passe au trousseau
- [x] Config éphémère `0600`, pas de secret en argv, `--no-ftm-push`
- [x] Parse / épinglage `trusted-cert`
- [x] Helper de commande redactée + journal
- [x] `tofv-helper` + Polkit (Connect/Couper sans sudo, après install)
- [x] Prévisualiser la commande
- [~] Tray (code prêt ; `libayatana-appindicator` requis pour l’icône)
- [x] Connect tray → popup TOTP (pas le panneau) ; retry si auth fail
- [x] Autostart XDG (`--tray`, pas d’autoconnect)
- [x] Lancement sans terminal (détache du TTY, `.desktop` `Terminal=false`)
- [x] Doctor bloquant si `openfortivpn` / `pppd` / trousseau / `pkexec` absents

---

## Flux utilisateur

Le daemon tray tourne tout seul (statut + actions rapides). Le panneau
ne s’ouvre que si on le demande.

### Premier lancement

1. Le tray apparaît : *disconnected*. Clic « Ouvrir l’UI ».
2. Dans le panneau : host, port, username, realm, mot de passe.
   Mode d’auth = TOTP (seul choix v1).
3. Enregistrer : le mot de passe va dans le trousseau (Secret Service), le reste dans
   `~/.config/tofv/profiles/default.toml`.
4. Clic Connecter (panneau ou tray) → **petite fenêtre TOTP**, 6 chiffres
   lus dans FortiToken (le panneau n’est pas obligatoire). Si le code est
   refusé, la fenêtre se rouvre tout de suite (F121, ~60 s).
5. TOFV écrit la config temporaire **sans** `trusted-cert` (premier essai),
   élève `openfortivpn`.
6. Si l’empreinte est inconnue : dialogue de confiance, sauvegarde, retry
   **avec le même TOTP** s’il est encore dans sa fenêtre de 30 s, sinon
   nouvelle popup.
7. Auth OK : tray → *connected*, helper montre l’interface `ppp` / gateway.

### Jours suivants

Tray (*disconnected*) → Connecter → popup TOTP → *connected*.
Le panneau n’est plus nécessaire.

### Certificat qui tourne

Si la passerelle change de cert, openfortivpn échoue à nouveau. TOFV
rouvre le dialogue de confiance (l’ancienne empreinte reste visible pour
comparaison) au lieu d’échouer silencieusement.

---

## Décisions déjà tranchées

| Décision | Choix | Pourquoi |
| --- | --- | --- |
| Langage cœur | Rust | Secrets + process privilégié-adjacent + shell Tauri |
| UI | Tauri 2 + TypeScript, tray-first | Front réel (helper, formulaires) sans Electron, Linux+macOS |
| Protocole VPN | Wrapper d’`openfortivpn`, pas de réimplémentation | Le binaire existe, est audité par l’usage, gère PPP/TLS |
| Secrets | Trousseau OS | Déverrouillé avec la session, pas un fichier maison |
| Mot de passe → openfortivpn | pinentry TOFV | Voie documentée, rien en argv |
| TOTP → openfortivpn | clé `otp` dans config temporaire | Contrat officiel « useful for a gui » |
| Élévation | Polkit Linux, sudo macOS, allowlist stricte | L’UI ne doit pas être root |
| Windows | non | Cahier des charges |
| Second facteur v1 | TOTP FortiToken 6 chiffres, saisi à la main | F121 / FTM : le seed n’est pas dans le QR ; pas de générateur |
| Push FTM / SAML | hors v1 (`--no-ftm-push` forcé) | Ne pas laisser openfortivpn basculer sur un push |
| Profils | un seul (`default`) en v1 | Le panneau reste simple ; multi-profils plus tard |
| Bureau de référence | Linux (Plasma d’abord) | SNI + Secret Service ; pas de KWallet en dur |
| Forme de l’app | daemon tray + bouton « Ouvrir l’UI » | Pas d’autoconnect : le statut est là, le Connect est un clic |
| Générateur TOTP | non | QR Fortinet = activation, pas un secret OATH |

---

## Installation

Trois chemins, **un seul à la fois**. Ne pas mélanger paquet distro et
`./scripts/install.sh` (deux helpers, deux `.desktop`, Polkit qui se
marche dessus).

Dans tous les cas TOFV n’est qu’un wrapper : **`openfortivpn` + `pppd`**
doivent exister sur la machine. Le doctor (`tofv doctor` et l’écran au
lancement) affiche la commande distro s’il manque quelque chose.

### 1. Paquet distro (utilisateur final)

Quand le paquet existe (Arch/CachyOS : `packaging/arch/PKGBUILD`) :

```sh
sudo pacman -S tofv
tofv-app          # ou double-clic dans Applications
```

Pacman tire `openfortivpn`, `ppp`, `libsecret`, le helper, la policy
Polkit et le lanceur Applications. **Pas** d’`install.sh` ensuite.
Mise à jour : `sudo pacman -Syu`. Désinstall : `sudo pacman -Rns tofv`
(le profil `~/.config/tofv/` et le trousseau restent).

### 2. Depuis les sources (dev, ou pas encore de paquet)

Prérequis : Podman, Node, et les paquets runtime
(`openfortivpn ppp libsecret polkit` — le doctor les rappelle).

```sh
git clone https://github.com/ange/tofv.git
cd tofv
./scripts/install.sh
tofv doctor
tofv-app          # détache du terminal ; pas besoin de le laisser ouvert
```

Ça compile en **release**, installe le helper dans `/usr/local/libexec`
(sudo), le `.desktop`, l’autostart et `~/.local/bin/tofv-app`. À chaque
`git pull` : rejouer `./scripts/install.sh`.

Pour itérer sans réinstaller : `./scripts/build-app.sh` puis
`./target/debug/tofv-app --foreground` (logs dans le terminal).

### 3. Archive de binaires (pas de paquet TOFV, pas de toolchain)

Prévu pour une machine qui a déjà `openfortivpn` mais pas de
`pacman -S tofv` et pas envie de compiler. **Pas encore publié** (ça
viendra avec un tag `vX.Y.Z` + artefact CI linux-x64).

Forme visée :

```sh
tar xf tofv-VERSION-linux-x64.tar.gz
cd tofv-VERSION
./install-bin.sh    # comme install.sh, sans l’étape cargo
tofv-app
```

Le tar contient `tofv-app`, `tofv`, `tofv-helper`, `pinentry-tofv`, la
policy Polkit, le `.desktop` et les icônes. Il **ne** contient **pas**
`openfortivpn`, `pppd`, GTK/WebKit ni `libsecret` : ceux-là restent des
paquets du système (ou déjà présents). Le helper est quand même copié
en root (`/usr/local/libexec` + policy). Binaire glibc x86_64 ; pas musl,
pas une vieille distro.

En attendant cette archive : chemin 2 (`./scripts/install.sh`).

### Prérequis runtime

- `openfortivpn` (preuve de vie en CLI avant TOFV)
- `pppd` (Linux)
- Secret Service : `secret-tool` / libsecret (KWallet, gnome-keyring, …)
- `pkexec` (polkit)
- WebKitGTK 4.1 + GTK3 pour la fenêtre
- optionnel : `libayatana-appindicator` pour le systray (sinon le panneau
  s’ouvre ; GNOME : extension AppIndicator)

`tofv doctor` (exit ≠ 0 si bloquant) et l’écran UI listent ce qui manque
et la commande `pacman` / `apt`.

### Lancer (sans garder un terminal)

Le process se détache tout seul s’il est lancé depuis un TTY. Fermer le
terminal (ou Ctrl+D) ne tue pas le tray. Une seconde invocation demande
à l’instance déjà là d’afficher le panneau, puis sort.

| Commande | Effet |
| --- | --- |
| `tofv-app` | panneau + tray, détache du terminal |
| `tofv-app --tray` | tray seulement (autostart au login, **pas** d’autoconnect) |
| `tofv-app --foreground` | reste dans ce terminal (logs, debug) |
| Double-clic TOFV dans Applications | `Terminal=false` — aucun terminal |
| Fermer la fenêtre | cache le panneau, le tray reste |

`~/.config/autostart/dev.tofv.desktop` (install.sh) ou
`/etc/xdg/autostart/dev.tofv.desktop` (paquet) lance `--tray` à la
session. Pas de connexion VPN automatique.

---

## Développement

Pas de Rust installé sur la machine hôte : la toolchain vit dans une image
Podman, la même qui servira plus tard à la CI GitHub.

```sh
# construit l'image au premier appel, puis lance cargo dedans
./scripts/cargo.sh test --workspace
./scripts/cargo.sh build --workspace
./scripts/cargo.sh clippy --workspace -- -D warnings

# panneau + tray (UI compilée sur l'hôte, binaire Rust dans Podman)
./scripts/build-app.sh
./target/debug/tofv-app --foreground

# install locale (release + helper Polkit + .desktop + autostart + ~/.local/bin)
./scripts/install.sh
tofv doctor
tofv-app
```

Le binaire debug charge `ui/dist` (pas le serveur Vite). Sans `npm run build`,
la fenêtre resterait vide.

Si le Containerfile change : `TOFV_REBUILD=1 ./scripts/build-app.sh`.

Paquets runtime (le doctor les affiche, le PKGBUILD Arch les tire tout seul) :
`openfortivpn`, `ppp`, `libsecret`, `polkit`. Systray : `libayatana-appindicator`
(sans ça, le panneau s’ouvre ; sur GNOME il faut l’extension AppIndicator).

### Helper root (sans mot de passe admin à chaque Connect/Couper)

L’UI ne tourne **jamais** en root. Un binaire allowlisté `tofv-helper` démarre/arrête
`openfortivpn` avec un argv figé (pas de `--pppd-plugin`). Polkit autorise la
session locale active **sans prompt**. `./scripts/install.sh` l’installe.

```sh
./scripts/cargo.sh run -p tofv-core --bin tofv -- doctor
./scripts/cargo.sh run -p tofv-core --bin tofv -- profile set \
    --host vpn.example.com --port 443 --username alice --realm corp
# mot de passe : stdin, jamais argv
printf '%s' 'secret' | ./target/debug/tofv profile password
./target/debug/tofv connect --otp 123456 --dry-run
./target/debug/tofv connect --otp 123456          # pkexec tofv-helper start
# si le cert est inconnu :
./target/debug/tofv trust <sha256>
./target/debug/tofv connect --otp 123456
```

Ordre d’implémentation :

1. `tofv-core` en CLI : profil, dry-run, parse `trusted-cert`, redact. **fait**
2. `pinentry-tofv` + keyring + runner live (`pkexec`, config `0600`). **fait**
3. Shell Tauri tray-only + panneau TypeScript. **en cours**
4. Autostart XDG + lancement sans terminal. **fait** — ensuite packaging / macOS.

---

## Licence

Le wrapper TOFV sera sous licence permissive (MIT ou Apache-2.0).
`openfortivpn` reste un *processus externe* GPL : on ne le linke pas, on
l’exécute. Les règles de sécurité ci-dessus (sudoers, pas de
`--pppd-plugin` exposé) restent notre responsabilité, pas la sienne.
