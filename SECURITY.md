# Security policy

TOFV runs a privileged helper and handles VPN credentials, so security
reports matter more here than feature requests.

## Reporting a vulnerability

Please **do not open a public issue** for a vulnerability.

Use GitHub's private reporting: *Security* → *Report a vulnerability* on
https://github.com/Altagen/TOFV. Include what you can reproduce, the
affected version or commit, and what an attacker gains.

Expect a first reply within a week. If a fix is needed, the advisory and the
patch are published together.

## Scope

In scope, and treated as vulnerabilities:

- Anything that lets an unprivileged local user get `tofv-helper` to act
  outside its contract — running a binary other than the allowlisted
  `openfortivpn`, accepting a config key outside the allowlist, reading or
  writing a file the calling user does not own, or leaking file content
  through an error message.
- Any path that puts the VPN password or the one-time code into a process
  argument, a persistent file, a log, or the clipboard.
- Any way for the TypeScript frontend to reach beyond the declared Tauri
  commands.
- Bypassing `trusted-cert` pinning, or getting `--insecure-ssl`,
  `--pppd-plugin` or `--pppd-log` through to a root `openfortivpn`.

Known and accepted, documented rather than hidden:

- **The pinentry socket is reachable by the same user.** While a connect
  attempt is in progress, a `0600` socket in the `0700` runtime directory
  hands the VPN password to a process that asks for it. The peer's uid is
  checked and the number of requests is capped, but another process running
  as *you* can still read it during that window. This is inherent to running
  `openfortivpn` as root while the UI is not; changing it means changing the
  architecture. Tracked as S-13 in [BACKLOG.md](BACKLOG.md).
- **The Polkit rule is `allow_active=yes`**, so Connect and Disconnect do not
  prompt. That is a deliberate trade for a personal VPN. Deployments that
  want a prompt should switch the action to `auth_admin_keep`.
- Anything an attacker who is already root can do.
- Vulnerabilities in `openfortivpn`, `pppd`, Polkit or the keyring
  implementation — report those upstream. If TOFV *amplifies* one of them,
  that is in scope here.

## Supported versions

TOFV is pre-1.0. Only the latest tag receives fixes.
