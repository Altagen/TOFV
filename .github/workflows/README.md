# Workflows

## `ci.yml` — on every push and pull request to `main` / `develop`

- **build, test, lint** — rustfmt, clippy with warnings as errors, the test
  suite, and a `--locked` release build.
- **dependency audit** — `cargo deny check advisories licenses bans sources`.
- **npm audit** — the frontend dependency tree.

## `security.yml` — Mondays, and on demand

Re-runs the advisory checks against unchanged code. CI alone only looks when
something is pushed, so a CVE published against a dependency nobody touched
would otherwise go unnoticed.

## `codeql.yml` — on push, PR, and Mondays

Static analysis of the TypeScript frontend and of the workflows themselves.

## Choices worth knowing

**Actions are pinned to commit SHAs, not tags.** A tag is mutable: whoever
controls the action repository can repoint `@v4` at new code, which then runs
in our CI with our token. That is not hypothetical — it is how the
`tj-actions/changed-files` compromise worked. Dependabot understands SHA pins
and raises PRs to bump them, so pinning does not mean going stale.

**Every workflow declares `permissions: contents: read`.** Without it the
`GITHUB_TOKEN` inherits the repository default, which can be read/write. Only
the CodeQL job asks for more (`security-events: write`, to upload results).

**`cargo deny` rather than `cargo audit` alone.** It covers advisories *and*
licences, banned crates and sources. The licence check is load-bearing here:
TOFV is MIT and only ever *executes* GPL `openfortivpn` as an external
process. `deny.toml` is what enforces that no GPL code is linked into the
binary, instead of the README merely asserting it. `[sources]` also refuses
any crate that does not come from crates.io.

**Tolerated advisories are listed individually in `deny.toml`, with a reason.**
Most are the archived gtk-rs GTK3 bindings that arrive through Tauri; TOFV
cannot drop them without dropping Tauri. They are ignored explicitly so the
list stays reviewable, rather than by turning the check off.

**CI installs the toolchain on `ubuntu-24.04` instead of reusing the Arch
`Containerfile`** that local builds use. The container exists so the binary
links like the Arch/CachyOS host it is developed on; CI only needs to know the
code compiles, tests and lints. The trade is real: an Arch-specific linking
problem will not show up here, so release artifacts (P2-L5) should be built
from the container.

**The runner's third-party apt sources are removed before `apt-get update`.**
GitHub images ship Chrome and Microsoft repositories we do not need, and
`apt-get update` exits non-zero whenever one of their mirrors is mid-sync —
which failed this project's very first CI run for reasons unrelated to it.
