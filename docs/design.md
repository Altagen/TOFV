# TOFV — design notes

Why this project exists, what `openfortivpn` actually gives us, why the
stack is what it is, and which decisions are already settled. The
[README](../README.md) covers installing and using TOFV; this file is the
reasoning behind it.

---

## What the alternatives do not cover

| Tool | Limitation for this brief |
| --- | --- |
| `openfortivpn` alone | No session memory, no tray, and the password is too easily passed in the clear (`-p` is visible in `ps`) |
| NetworkManager-fortisslvpn | Official wrapper, but OTP/SAML handling is uneven and you get little visibility into the command actually run |
| Proprietary FortiClient | Heavy, a poor Linux citizen, and not a wrapper around the free binary |
| [openfortivpn-webview](https://github.com/gm-vm/openfortivpn-webview) | Only useful for fetching a SAML cookie |

TOFV is deliberately **not**:

- a new VPN client — the tunnel, TLS, PPP, routes and DNS all stay with
  `openfortivpn` and `pppd`;
- a homemade secret store — Linux uses the **Secret Service** API
  (`secret-tool` / libsecret), so KWallet, gnome-keyring or anything else
  compatible works; macOS will use the Keychain behind the same
  `PasswordStore` trait;
- a Windows application;
- a NetworkManager replacement — TOFV coexists with it, it does not plug
  into it.

---

## What matters about openfortivpn

Sources: the [official README](https://github.com/adrienverge/openfortivpn/blob/master/README.md)
and [man openfortivpn(1)](https://manpages.debian.org/testing/openfortivpn/openfortivpn.1.en.html).

`openfortivpn` establishes a **PPP over TLS** tunnel to a Fortinet gateway.
It spawns `pppd`, then configures routes and DNS. It is packaged nearly
everywhere: Debian/Ubuntu, Fedora, Arch, NixOS, Homebrew, MacPorts.

### Options that matter to a GUI

| Need | CLI option | Config key | Note |
| --- | --- | --- | --- |
| Gateway | `host[:port]` | `host`, `port` | Commonly `443` or `8443` |
| Login | `-u` / `--username` | `username` | |
| Password | `-p` / `--password` | `password` | **Never pass in argv** — readable in `ps`. Use pinentry or a temporary `0600` config |
| Realm | `--realm` | `realm` | Often empty; required on some deployments |
| OTP | `-o` / `--otp` | `otp` | The man page says `otp =` is explicitly "useful for a gui" |
| OTP prompt | `--otp-prompt` | `otp-prompt` | When the server does not use the default prompt |
| OTP delay | `--otp-delay` | `otp-delay` | When the token is not valid yet |
| Disable FTM push | `--no-ftm-push` | `no-ftm-push` | Forces the OTP path instead of a FortiToken Mobile notification |
| Gateway certificate | `--trusted-cert` | `trusted-cert` | **SHA-256** digest of the DER X.509 certificate. Repeatable |
| Password via pinentry | `--pinentry=` | `pinentry` | The officially "secure" path |
| Config file | `-c` / `--config` | — | Intended for GUIs. CLI flags override the file |
| SAML cookie | `--cookie` / `--cookie-on-stdin` | — | For browser/webview SSO |
| Native SSO | `--saml-login` | — | Small local server (default `:8020`) that catches the SAML redirect |
| Client certificate | `--user-cert` / `--user-key` | `user-cert`, `user-key` | PEM or `pkcs11:` (YubiKey and friends) |
| Reconnect | `--persistent=` | `persistent` | Infinite loop, interval in seconds |
| Verbosity | `-v` / `-q` | | TOFV passes `-v` to feed the log view |

Deliberately **not exposed** in the UI: `--pppd-plugin`, `--pppd-log`,
`--insecure-ssl`. The openfortivpn README says it plainly — a malicious user
who can pass `--pppd-plugin` to a root `openfortivpn` has arbitrary code
execution. TOFV must never relay those flags from the UI, and `tofv-helper`
rejects them outright.

> **One caveat about `-v`.** At verbosity level DEBUG, `openfortivpn` logs
> one line **per packet** from inside its packet-forwarding threads, and its
> logger calls `fflush()` while holding a global mutex. If whatever reads its
> stdout falls behind, the 64 KiB pipe fills, the logger blocks, and all four
> forwarding threads stall with it — the tunnel freezes. Anything TOFV does
> per log line must therefore cost constant time. See
> `ParseState::ingest` in `crates/tofv-core/src/runner.rs`.

### Trusting the gateway certificate

Many Fortinet gateways present a certificate the system PKI will not
validate: self-signed, incomplete chain, or a mismatched name.
`openfortivpn` then refuses and prints:

```
ERROR: Gateway certificate validation failed, and the certificate digest is not in the local whitelist.
If you trust it, rerun with:
    --trusted-cert <sha256>
```

That is **the** first-run flow. TOFV parses that error, shows the
fingerprint with host and port, saves it into the profile once confirmed,
and retries. You can also compute it up front:

```sh
echo | openssl s_client -connect vpn.example.com:443 2>/dev/null \
  | openssl x509 -outform DER | sha256sum
```

`--insecure-ssl` is not an acceptable alternative.

### The password: three paths, one correct for a GUI

1. `-p password` — forbidden. Visible in `/proc/<pid>/cmdline`.
2. `password =` in a persistent file — forbidden. It survives, gets backed
   up, and leaks to an unlucky copy/paste.
3. **pinentry, or an ephemeral `0600` config** — the TOFV path.

The man page recommends pinentry (`pinentry-gnome3`, `pinentry-qt`,
`pinentry-mac`). For a GUI that *already holds* the secret in the keyring,
the cleanest option is a **TOFV pinentry**: a tiny binary that speaks the
pinentry protocol and returns the keyring password, with no extra dialog.
The OTP is different — it is written into a temporary config file when you
click Connect (the official GUI contract) and the file is destroyed after.

### Privileges: openfortivpn must be root, the UI must not

`openfortivpn` needs root at three moments: spawning `/usr/sbin/pppd`,
installing routes once the tunnel is up, and writing nameservers
(`/etc/resolv.conf` or `resolvconf`).

The architectural consequence: **the tray process runs as the user** and
only the `openfortivpn` child is elevated.

| Mechanism | Platform | Comment |
| --- | --- | --- |
| Polkit (`pkexec` + a `.policy` rule) | Linux | Best UX: graphical dialog, scopeable. Preferred |
| A dedicated sudoers entry with a **fixed** command | Linux / macOS | Acceptable only as an allowlist with no dangerous wildcards |
| Typing the sudo password every time | anywhere | Safety net, poor UX |

An open `ALL=(ALL) NOPASSWD: /usr/bin/openfortivpn` is a vulnerability
(`--pppd-plugin`). TOFV's Polkit rule points at `tofv-helper` only, which
accepts nothing but `start --config <path under /run/user/$UID/tofv/>` and
`stop`.

### The one-time code

The reference gateway asks for a 6-digit token code with a 30 s period,
read from an authenticator app or a hardware token. It is not a push, not
SAML, not an emailed code.

The real sequence on the `openfortivpn` side is username + password, and
**then** the server asks for the token. So the code must be supplied
*before* openfortivpn authenticates — waiting until the tunnel is up is too
late. TOFV therefore:

1. asks for the code when you click Connect (tray or panel);
2. writes `otp = <code>` into the temporary `0600` config;
3. elevates `openfortivpn -c … --pinentry=…`;
4. destroys the temporary config.

`--no-ftm-push` is passed by default so a mobile push cannot short-circuit
the code field.

There is no built-in code generator: the enrolment QR code is an activation
payload, not an OATH seed (`otpauth://`). The secret stays in the token.
SAML, cookies and `--saml-login` are out of scope for v1.

### macOS

`openfortivpn` is in Homebrew and MacPorts. On recent macOS the Apple
`pppd` is old, and the official README recommends `--enable-legacy-pppd`
when building from source. The wrapper itself changes nothing about the
protocol, but macOS packaging will need to check for the binary, run as
`LSUIElement` / `ActivationPolicy::Accessory` (no Dock icon), use the
Keychain, and elevate through sudo rather than Polkit.

---

## Stack choice

The brief is not "pick the trendiest thing", it is **secret hygiene, a small
Unix tray daemon, and a UI that explains what it is doing**. The language
matters less than the architecture — no password in argv, a non-root UI, a
strict elevation allowlist. Once that is fixed, the question is which tool
makes that architecture natural.

| Criterion | Weight | Rust | Go |
| --- | --- | --- | --- |
| Memory hygiene around secrets (zeroize, no easy phantom copies) | high | better | fine, but the GC holds strings longer |
| Capability model for a desktop UI | high | Tauri 2 has a native ACL model | Wails / Fyne: more permissive by default |
| Linux tray (StatusNotifier / AppIndicator) + macOS `NSStatusItem` | high | Tauri 2, `tray-icon` | `fyne.io/systray`, Fyne |
| Log/command window, profile forms | high | system webview, TypeScript frontend | Fyne (native, poorer) or Wails (webview) |
| Keyring integration (libsecret / Keychain) | high | mature `keyring` crate | `zalando/go-keyring`, also mature |
| Process supervision, stderr parsing, `0600` files | medium | excellent | excellent, often shorter |
| Weight of the "small daemon" | medium | Tauri pulls WebKitGTK on Linux | Fyne / systray lighter; Wails comparable to Tauri |
| Build chain | medium | rustc plus Node for Tauri | a single `go build` |
| Industry precedent (desktop VPN client) | low | Firezone picked Tauri for Linux | many Go trays (syncthing, …) |

Go is not a bad choice. A `systray` daemon plus a small Fyne window would
reach a rough MVP faster, as a single binary with no WebKit. What loses it
here is that the brief asks for a real **front end** — command detail,
profile forms, log view — and Fyne forces all of that into Go with a poorer
result; Wails copies Tauri without the permission model; and for a program
that touches sudo, a password and a one-time code, a sandboxed local
frontend (CSP, IPC allowlist) is worth the overhead.

### Decision

**Rust + Tauri 2 + TypeScript**, tray-first.

- **Rust** carries the core: profiles, keyring, temporary config generation,
  spawning and supervising `openfortivpn`, parsing `trusted-cert` errors,
  redacted logging.
- **Tauri 2** is the desktop frame: official Linux/macOS tray, on-demand
  windows, typed IPC, and an ACL so the frontend cannot write a sudoers file
  or run an arbitrary argv.
- **TypeScript** for the panel, the code prompt, the certificate dialog and
  the log view — the right tool for forms and logs, with a deliberately thin
  UI layer.

Tauri here is *not* an Electron app, *not* a permanent window (the process
starts tray-only and windows open on demand), and *not* an excuse to run a
webview as root.

### Alternatives explicitly rejected

| Option | Why not, for *this* project |
| --- | --- |
| Go + Fyne | Lighter, but the UI is too poor for the log view and the certificate/OTP flows |
| Go + Wails | Same shape as Tauri with weaker IPC/ACL guarantees |
| Rust + iced / egui | All-Rust UI, but weaker tray and accessibility, and longer to polish |
| Rust with `tray-icon` and no window | Too small — loses the log/command view the brief asks for |
| Electron | Wrong target (weight, surface, Windows-first) |
| A NetworkManager plugin | Loses control of the command, and OTP is already its weak point |

If WebKitGTK ever becomes a real problem on a minimal distro, `tofv-core`
stays reusable behind another shell. That is why the core is **not** coupled
to Tauri.

---

## Architecture

```
TOFV/
├── crates/
│   ├── tofv-core/       # profiles, secrets, spawn, parse, redact
│   ├── tofv-helper/     # the only binary Polkit/sudo may run as root
│   └── pinentry-tofv/   # pinentry helper that reads the keyring
├── src-tauri/           # Tauri 2 shell: tray, windows, IPC
├── ui/                  # TypeScript frontend
└── packaging/
    ├── arch/            # PKGBUILD
    └── linux/           # .desktop, icons, polkit, sudoers example
```

- **`tofv-core`** does not depend on Tauri, and is exercised by the `tofv` CLI.
- **`tofv-helper`** is the privileged piece. It validates the config on the
  file descriptor it reads from, refuses any key outside a small allowlist,
  copies the result into a root-owned `0600` file, and execs `openfortivpn`
  with a fixed argv.
- **`pinentry-tofv`** is tiny and is invoked by root `openfortivpn`. It
  receives no secret as an argument; it fetches one from the user session
  over a `0600` unix socket.
- **`src-tauri`** is only the shell: tray, windows, and a narrow IPC surface.
- **`ui`** has no filesystem or process access outside that API.

### Persistent data

```
~/.config/tofv/config.toml          # UI preferences, active profile
~/.config/tofv/profiles/<id>.toml   # host, port, realm, trusted-cert, username
                                    # NEVER the password, NEVER the OTP
```

Secrets live in the keyring under service `dev.tofv`, with the profile id as
the account. Runtime files live under `/run/user/$UID/tofv/` (`0600`,
`O_EXCL`): the ephemeral openfortivpn config, the pinentry socket, and the
single-instance socket.

### The log view

It is not cosmetic. It shows the command that actually ran (already
redacted), the contents of the temporary config with `otp = ******`, live
stdout/stderr from openfortivpn filtered for anything resembling a password,
an `SVPNCOOKIE` or an OTP, and TOFV's own state. "Copy command" copies the
redacted form, never the secrets.

---

## Decisions already settled

| Decision | Choice | Why |
| --- | --- | --- |
| Core language | Rust | Secrets plus a privilege-adjacent process, plus the Tauri shell |
| UI | Tauri 2 + TypeScript, tray-first | A real front end without Electron, Linux and macOS |
| VPN protocol | Wrap `openfortivpn`, do not reimplement | The binary exists, is battle-tested, and handles PPP/TLS |
| Secrets | OS keyring | Unlocked with the session, not a homemade file |
| Password → openfortivpn | TOFV pinentry | Documented path, nothing in argv |
| OTP → openfortivpn | `otp` key in the temporary config | The official "useful for a gui" contract |
| Elevation | Polkit on Linux, sudo on macOS, strict allowlist | The UI must not be root |
| Windows | no | Out of scope |
| Second factor for v1 | 6-digit token code, typed by hand | The enrolment QR is not an OATH seed |
| FTM push / SAML | out of scope for v1 (`--no-ftm-push` forced) | Do not let openfortivpn switch to a push |
| Profiles | one (`default`) in v1 | Keeps the panel simple; multi-profile later |
| Reference desktop | Linux, Plasma first | StatusNotifier + Secret Service, no hardcoded KWallet |
| Application shape | tray daemon plus an "open the panel" action | No autoconnect: status is always there, connecting is a click |
| Built-in code generator | no | The Fortinet QR is activation data, not an OATH secret |
