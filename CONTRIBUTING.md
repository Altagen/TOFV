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
`main` is tagged with a bare version — `0.1.0`, no `v` prefix. That matches
the other Altagen projects and is what the Ora registry entry substitutes into
the asset name, so a `v` would break `ora install tofv`.

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

## Cutting a release

Tagging is the whole procedure — `release.yml` does the rest — but the tag is
also the point of no return, so the checks come first.

1. Bump the version in **all four** manifests: `Cargo.toml` (workspace),
   `src-tauri/tauri.conf.json`, `ui/package.json`, `packaging/arch/PKGBUILD`.
   Then confirm:

   ```sh
   ./scripts/check-version.sh 0.2.0
   ```

   The release workflow runs this before building, so a mismatch fails in
   seconds rather than after a full compile. It matters because the binaries
   report the Cargo version through `--version`.

2. Merge `develop` into `main`, then tag `main` with a **bare** version:

   ```sh
   git tag 0.2.0 && git push origin 0.2.0
   ```

3. The workflow builds `linux-x86_64`, stages the tarball, generates a
   CycloneDX SBOM, writes `SHA256SUMS.txt`, builds the notes from the
   Conventional Commits history with git-cliff, and publishes. A tag with a
   suffix (`0.2.0-rc.1`) is published as a pre-release.

If something fails **after** the release exists, delete it and the tag before
retrying — `gh release delete 0.2.0 --cleanup-tag` — otherwise the second run
collides with the assets of the first.

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
