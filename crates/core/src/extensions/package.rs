use super::*;
use base64::engine::general_purpose::STANDARD;
use media_extension_sdk::{API_VERSION, MAX_MODULE, Package};
use ring::signature::{ED25519, UnparsedPublicKey};
use sha2::{Digest, Sha256};

pub fn validate_package(package: &Package, public_key: &[u8]) -> Result<(), &'static str> {
    let m = &package.manifest;
    if m.api_version != API_VERSION {
        return Err("incompatible_api");
    }
    if !identifier(&m.id)
        || !identifier(&package.publisher_id)
        || m.name.is_empty()
        || m.name.len() > 120
        || m.description.len() > 2048
        || m.author.is_empty()
        || m.author.len() > 120
        || m.license.is_empty()
        || m.license.len() > 80
        || m.repository.len() > 512
        || !m.repository.starts_with("https://")
        || m.version.split('.').count() != 3
        || m.version
            .split('.')
            .any(|s| s.is_empty() || s.len() > 8 || !s.bytes().all(|b| b.is_ascii_digit()))
        || m.permissions.is_empty()
        || m.http_hosts.len() > 16
        || m.http_hosts.iter().any(|host| !http_host(host))
        || m.permissions.iter().any(|p| {
            ![
                "metadata.read",
                "metadata.write",
                "avatars",
                "images",
                "providers",
                "storage.private",
                "playback.markers",
                "subtitles",
                "requests",
                "reports",
            ]
            .contains(&p.as_str())
        })
    {
        return Err("invalid_manifest");
    }
    if package.wasm.len() > MAX_MODULE * 4 / 3 + 4 {
        return Err("module_too_large");
    }
    if let Some(migration) = &m.storage_migration {
        let valid_key = |key: &str| !key.is_empty() && key.len() <= 256;
        if migration.from_version.len() > 32
            || migration.key_renames.len() > 32
            || migration
                .key_renames
                .iter()
                .enumerate()
                .any(|(index, rename)| {
                    !valid_key(&rename.from)
                        || !valid_key(&rename.to)
                        || rename.from == rename.to
                        || migration
                            .key_renames
                            .iter()
                            .any(|other| other.from == rename.to)
                        || migration.key_renames[..index]
                            .iter()
                            .any(|other| other.from == rename.from || other.to == rename.to)
                })
        {
            return Err("invalid_storage_migration");
        }
    }
    let wasm = STANDARD
        .decode(&package.wasm)
        .map_err(|_| "invalid_module")?;
    if wasm.len() > MAX_MODULE || !wasm.starts_with(b"\0asm") {
        return Err("invalid_module");
    }
    // A module over the 1.0 limit only runs on hosts of ABI 1.1, which its package must declare
    // so that older Cores never offer it.
    if wasm.len() > media_extension_sdk::MAX_MODULE_V1_0
        && !super::repositories::accepts_large_input(m.target_abi.as_deref())
    {
        return Err("module_too_large");
    }
    if package.sha256 != format!("{:x}", Sha256::digest(&wasm)) {
        return Err("invalid_digest");
    }
    let signature = STANDARD
        .decode(&package.signature)
        .map_err(|_| "invalid_signature")?;
    let signed = package.signing_bytes().map_err(|_| "invalid_manifest")?;
    UnparsedPublicKey::new(&ED25519, public_key)
        .verify(&signed, &signature)
        .map_err(|_| "invalid_signature")
}

// Validate the same bounded module envelope used by the worker before any
// prospective version switch; signature and digest have already been checked.
pub(super) fn validate_update_module(package: &Package) -> Result<(), &'static str> {
    use wasmi::{Config, EnforcedLimits, Engine, FuncType, Module, ValType};
    let bytes = STANDARD
        .decode(&package.wasm)
        .map_err(|_| "invalid_module")?;
    let mut config = Config::default();
    config.consume_fuel(true).allow_start_fn(false);
    config.enforced_limits(EnforcedLimits::strict());
    config.set_max_stack_height(64 * 1024);
    config.set_max_recursion_depth(128);
    let engine = Engine::new(&config);
    let module = Module::new(&engine, &bytes).map_err(|_| "invalid_module")?;
    let host_signature = FuncType::new([ValType::I32, ValType::I32], [ValType::I64]);
    for import in module.imports() {
        let allowed = import.module() == "aether" && import.name() == "aether_host_http"
            || import.module() == "media"
                && matches!(
                    import.name(),
                    "media_host_storage_v1" | "media_host_objects_v1"
                );
        if !allowed || import.ty().func() != Some(&host_signature) {
            return Err("imports_forbidden");
        }
    }
    let alloc = FuncType::new([ValType::I32], [ValType::I32]);
    let run = FuncType::new([ValType::I32, ValType::I32], [ValType::I64]);
    if module
        .get_export("media_alloc")
        .and_then(|e| e.func().cloned())
        != Some(alloc)
        || module
            .get_export("media_run")
            .and_then(|e| e.func().cloned())
            != Some(run)
        || !module
            .get_export("memory")
            .and_then(|e| e.memory().copied())
            .is_some_and(|m| !m.is_64() && m.minimum() <= 256)
    {
        return Err("incompatible_abi");
    }
    Ok(())
}
