# Releasing

Version 0.0.1 is published on crates.io, as a GitHub release with the
`x86_64-unknown-linux-musl` asset, and as a standalone skill release. No model is
bundled.

For a later release, work from `main` with a clean tree and the committed
`Cargo.lock`. Update the version used by `Cargo.toml`, `Cargo.lock`, the changelog,
skills, installer, and commands below.

Before any irreversible publication:

1. Run `make fmt-check`, `make test`,
   `LIBCLANG_PATH=/usr/lib/llvm-10/lib cargo clippy --locked --all-targets --all-features -- -D warnings`,
   `LIBCLANG_PATH=/usr/lib/llvm-10/lib make test-full`, `make verify`, and
   `make smoke`. Adjust `LIBCLANG_PATH` for the host toolchain.
2. Run the benchmarks needed for release claims and record host and cache
   conditions. Native model evaluation is useful evidence, but it is not a blocker
   for the default model-free crate.
3. Run `cargo package --locked`, inspect the file list and size, and test the
   extracted crate before publication:

   ```sh
   cargo install --locked --path target/package/watf-0.0.1 \
     --root target/install-smoke --force
   test "$(target/install-smoke/bin/watf --version)" = "watf 0.0.1"
   ```

4. Confirm the two skill copies are byte-identical and review the standalone
   skill README and license.

The following publication steps are irreversible or externally visible. A
maintainer must run them explicitly for the new version:

1. Publish the crate with `cargo publish --locked`.
2. After crates.io indexing completes, test the published crate:

   ```sh
   cargo install watf --version 0.0.1 --locked \
     --root target/crates-io-install-smoke
   test "$(target/crates-io-install-smoke/bin/watf --version)" = "watf 0.0.1"
   ```

3. Push the matching tag from `main`. The tag workflow builds a model-free
   `x86_64-unknown-linux-musl` binary plus source and skill archives.
4. Publish `watf-skills` to its standalone repository after independent review.
5. Deploy `site/` through operator-managed infrastructure, then perform a real
   public `GET` of `https://watf.sanixdk.xyz/` and confirm the 0.0.1 page content.

Full local-model builds and additional targets remain maintainer builds. Release
binaries must not use `target-cpu=native`. The tag workflow cross-builds on Ubuntu
22.04 with musl tools. It requires an x86-64 ELF and rejects an ELF interpreter,
dynamic-library dependencies, and GLIBC version references. The published asset
passed a release-session smoke test on a glibc 2.31 host. This evidence covers
that asset only; it does not establish broad Linux, macOS, or Windows compatibility.
Release Drafter targets `main`.
