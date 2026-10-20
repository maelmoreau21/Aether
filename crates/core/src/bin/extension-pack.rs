//! Offline signing utility. Keys never go through the server or browser.
use base64::{Engine as _, engine::general_purpose::STANDARD};
use media_extension_sdk::{Manifest, Package};
use ring::{
    rand::SystemRandom,
    signature::{Ed25519KeyPair, KeyPair},
};
use sha2::{Digest, Sha256};
use std::{io::Write, path::Path};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() == 3 && args[1] == "keygen" {
        let key = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new())
            .map_err(|_| "key generation failed")?;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        options.open(&args[2])?.write_all(key.as_ref())?;
        let pair = Ed25519KeyPair::from_pkcs8(key.as_ref()).map_err(|_| "invalid key")?;
        println!("{}", STANDARD.encode(pair.public_key().as_ref()));
        return Ok(());
    }
    if args.len() != 7 || args[1] != "pack" {
        return Err("Usage: media-extension-pack keygen KEY.pk8 | pack MANIFEST.json MODULE.wasm KEY.pk8 PUBLISHER_ID OUTPUT.json".into());
    }
    let wasm = std::fs::read(&args[3])?;
    let manifest: Manifest = serde_json::from_slice(&std::fs::read(&args[2])?)?;
    let key_bytes = zeroize::Zeroizing::new(std::fs::read(&args[4])?);
    let key = Ed25519KeyPair::from_pkcs8(&key_bytes).map_err(|_| "invalid key")?;
    let mut package = Package {
        manifest,
        publisher_id: args[5].clone(),
        wasm: STANDARD.encode(&wasm),
        sha256: format!("{:x}", Sha256::digest(&wasm)),
        signature: String::new(),
    };
    package.signature = STANDARD.encode(key.sign(&package.signing_bytes()?).as_ref());
    media_poc::extensions::validate_package(&package, key.public_key().as_ref())
        .map_err(|_| "invalid extension package")?;
    let output = Path::new(&args[6]);
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?
        .write_all(&serde_json::to_vec_pretty(&package)?)?;
    Ok(())
}
