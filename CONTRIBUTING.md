# Contributing to TOFV

## Getting a build

There is no Rust toolchain on the host: it lives in a Podman image built
from the `Containerfile`. The UI is built on the host with Node.

```sh
npm --prefix ui install
npm --prefix ui run build      # the Rust build embeds ui/dist
./scripts/cargo.sh test --workspace
./scripts/cargo.sh clippy --workspace -- -D warnings
./scripts/cargo.sh fmt --all -- --check
```

`./scripts/build-app.sh` does the UI build and the Rust build together, and
`./target/debug/tofv-app --foreground` runs the app with logs in the
terminal. If you change the `Containerfile`, rebuild the image with
`TOFV_REBUILD=1 ./scripts/build-app.sh`.

## Branching and commits

GitFlow: work branches off `develop`, `develop` merges into `main`, and
`main` is tagged `vX.Y.Z`.

Commit messages follow [Conventional Commits](https://www.conventionalcommits.org)
so release notes can be generated from the history:

```
feat(ui): show the ppp0 address once the tunnel is up
fix(helper): reject a config whose parent directory changed mid-read
docs: explain why -v makes per-line cost a correctness concern
```

Common types: `feat`, `fix`, `perf`, `refactor`, `docs`, `test`, `ci`,
`build`, `chore`. Use `!` or a `BREAKING CHANGE:` trailer for anything that
changes the profile format, the IPC surface or the helper contract.

## What a change needs

- **Tests for anything with a security or performance contract.** The two
  that matter most are `reader_keeps_up_with_per_packet_debug_flood` (log
  ingestion must not become a function of session length, or the tunnel
  stalls) and the helper's `swapping_the_file_after_the_check_cannot_read_another_users_file`.
  If you touch either area, the test should fail before your fix.
- **No new secret paths.** The password and the one-time code must never
  reach a process argument, a persistent file, a log line or the clipboard.
  `crates/tofv-core/src/redact.rs` is the last line of defence, not the
  first.
- **Nothing new through the helper's argv.** `tofv-helper` accepts
  `start --config <path>` and `stop`. Widening that contract needs a very
  good reason and a review of `crates/tofv-helper/src/validate.rs`.
- **New Tauri commands are a capability change.** Adding one widens what the
  frontend can reach; say so in the PR.

## Before opening a PR

```sh
./scripts/cargo.sh test --workspace
./scripts/cargo.sh clippy --workspace -- -D warnings
./scripts/cargo.sh fmt --all -- --check
npm --prefix ui run build      # runs tsc --noEmit
```

Found a vulnerability? Do not open a PR or an issue — see
[SECURITY.md](SECURITY.md).
