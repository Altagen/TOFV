# Workflows

`ci.yml` runs on every push and pull request touching `main` or `develop`:
rustfmt, clippy with warnings as errors, the test suite, and a release
build — plus `cargo audit` for advisories in the dependency tree.

CI installs the toolchain directly on `ubuntu-24.04` rather than reusing the
Arch `Containerfile` that local builds use. The `Containerfile` exists so the
binary links the same way as the Arch/CachyOS host it is developed on; CI only
needs to know the code compiles, passes its tests and is lint-clean, and a
plain runner does that in a fraction of the time.

That difference is deliberate but not free: a linking problem specific to Arch
would not show up here. Release artifacts, when they exist (P2-L5), should be
built from the `Containerfile`.
