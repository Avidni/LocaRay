use std::{env, fs::File, io::Read, path::PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};

#[derive(Deserialize)]
struct SidecarManifest {
    target: String,
    sha256: String,
}

fn main() {
    verify_sidecar();
    tauri_build::build();
}

fn verify_sidecar() {
    let manifest_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| "src-tauri".to_owned()));
    let manifest_path = manifest_dir.join("binaries/checksums.json");
    println!("cargo:rerun-if-changed={}", manifest_path.display());

    let manifest_bytes = std::fs::read(&manifest_path)
        .unwrap_or_else(|error| panic!("sidecar checksum manifest could not be read: {error}"));
    let manifest: SidecarManifest = serde_json::from_slice(&manifest_bytes)
        .unwrap_or_else(|error| panic!("sidecar checksum manifest is invalid: {error}"));
    let binary_path = manifest_dir.join("binaries").join(&manifest.target);
    println!("cargo:rerun-if-changed={}", binary_path.display());

    let mut binary = File::open(&binary_path)
        .unwrap_or_else(|error| panic!("bundled cloudflared binary is missing: {error}"));
    let mut digest = Sha256::new();
    let mut chunk = vec![0_u8; 64 * 1024];
    loop {
        let read = binary.read(&mut chunk).unwrap_or_else(|error| {
            panic!("bundled cloudflared binary could not be read: {error}")
        });
        if read == 0 {
            break;
        }
        digest.update(&chunk[..read]);
    }
    let actual = format!("{:x}", digest.finalize());
    assert_eq!(
        actual, manifest.sha256,
        "bundled cloudflared checksum verification failed"
    );
}
