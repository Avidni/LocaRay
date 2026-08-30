$ErrorActionPreference = "Stop"

$manifestPath = Join-Path $PSScriptRoot "..\src-tauri\binaries\checksums.json"
$manifest = Get-Content -Raw -LiteralPath $manifestPath | ConvertFrom-Json
$targetPath = Join-Path (Split-Path $manifestPath) $manifest.target

if (-not (Test-Path -LiteralPath $targetPath)) {
    Invoke-WebRequest -Uri $manifest.source -OutFile $targetPath
}

$actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $targetPath).Hash.ToLowerInvariant()
if ($actual -ne $manifest.sha256) {
    throw "cloudflared checksum verification failed."
}

Write-Host "Verified cloudflared $($manifest.version): $actual"
