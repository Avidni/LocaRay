# Bundled sidecar

The Windows x64 release bundles `cloudflared` 2026.8.2 from the official
Cloudflare GitHub release. The expected SHA-256 digest is recorded in
`checksums.json` and verified by `scripts/prepare-sidecar.ps1` before packaging.

The executable is never self-updated and must not be replaced outside the
controlled release process.
