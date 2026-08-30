# LocaRay

LocaRay gives a local development server a temporary public HTTPS URL through
a Cloudflare Quick Tunnel. It bundles the tunnel component, detects local
development ports, and keeps tunnel lifecycle control in a native Windows app.

## Download

[Download the latest Windows installer](https://github.com/Avidni/LocaRay/releases/latest)

LocaRay 0.x builds are unsigned community previews. Windows may show an
unknown-publisher warning. Download only from this repository's Releases page
and verify the SHA-256 value published with each release.

## Use

1. Start your local project normally, for example with `pnpm dev`.
2. Open LocaRay and select or enter the local port.
3. Select **Start tunnel**.
4. Copy the validated URL or scan its QR code.
5. Stop the tunnel when sharing is finished.

Anyone with the generated URL may access the local service while the tunnel is
running. LocaRay is for development and testing, not production hosting.

## Privacy and security

- Tunneling is handled by a version-pinned, checksum-verified `cloudflared`
  sidecar downloaded from the official Cloudflare release.
- LocaRay does not upload tunneled content, credentials, ports, URLs, IP
  addresses, or raw process output by default.
- Tunnel URLs and QR codes remain local.
- The sidecar cannot self-update independently.

Read the [Cloudflare Quick Tunnel documentation](https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/do-more-with-tunnels/trycloudflare/)
before exposing sensitive development services.

## Build from source

Requirements: Windows 10 or 11 x64, Node.js, pnpm 10.19.0, and the Rust toolchain
pinned in `rust-toolchain.toml`.

```powershell
pnpm install --frozen-lockfile
.\scripts\prepare-sidecar.ps1
pnpm format:check
pnpm lint
pnpm typecheck
pnpm test
pnpm test:e2e
pnpm tauri build
```

The sidecar preparation script downloads only the URL pinned in
`src-tauri/binaries/checksums.json` and verifies its SHA-256 digest. Windows
installers are produced under `src-tauri/target/release/bundle/`.

## License

LocaRay source code is available under the [MIT License](LICENSE).
The bundled `cloudflared` executable is distributed under Apache License 2.0;
third-party notices ship with the application.
