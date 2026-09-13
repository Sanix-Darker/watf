# Releasing

No public repository or endpoint is assumed. Before enabling a curl install URL:

1. Create the actual repository and review licenses and provenance.
2. Run `make fmt`, generate a real `Cargo.lock`, review it, and commit both changes.
3. Run core/native tests, the CLI smoke script and the offline verifier.
4. Run opt-in native model evaluation on supported hardware and review failures.
5. Run representative benchmarks and record hardware/cache conditions.
6. Set the version in Cargo.toml, update the changelog and installer default.
7. Push a matching `vX.Y.Z` tag. Review the generated draft before publishing it.

`release.yml` uses one resolved lockfile across its Linux x86_64/aarch64 and
lite/full matrix. It tests each build, packages a flat archive, emits individual
SHA256 files and creates a release draft. The model is not bundled. Binary assets
include source licenses, corpus/manifest, skill, model manifest, Cargo.lock and
build metadata. Dynamic library requirements are separate release assets.

Portable releases must not set `target-cpu=native`. GNU/Linux x86_64 is built on
Ubuntu 22.04; aarch64 uses Ubuntu 24.04. Inspect resulting library requirements,
and test on your actual minimum target OS before claiming compatibility. These
runner choices do not prove support for every Linux distribution or older glibc.

The initial source archive has no Cargo.lock because dependency resolution could
not run in its assembly environment. CI can bootstrap one and preserve it as an
artifact. This is a starting aid, not a substitute for committing a reviewed lock
before a production release. Cargo manifests pin the native wrapper/sys pair;
other dependency resolution is not frozen until a real lock is generated.

Release-drafter maintains change notes on main. The tagged build workflow creates
a versioned binary draft separately. Review or remove an older notes-only draft
when publishing to avoid leaving duplicate drafts. Dependabot groups native
wrapper/sys updates, but maintainers must still inspect their ABI/API compatibility.

Regenerate the standalone `watf-skills` ZIP from the same canonical SKILL.md and
keep protocol version compatibility explicit. No workflow publishes an unrelated
skill repository or modifies user agent configuration automatically.
