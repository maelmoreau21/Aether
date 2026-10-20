//! Extension API v1. No filesystem, network or host imports.
use serde::{Deserialize, Serialize};

pub const API_VERSION: u32 = 1;
pub const MAX_MODULE: usize = 262_144;
pub const MAX_JSON: usize = 32_768;

/// Generic ABI envelope. Host adapters authorize operations before invocation.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExtensionRequest {
    pub api_version: u32,
    pub operation: String,
    pub input: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExtensionResponse {
    pub api_version: u32,
    pub result: serde_json::Value,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConfigOption {
    pub label: String,
    pub value: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConfigField {
    pub key: String,
    pub label: String,
    #[serde(default)]
    pub description: String,
    pub field_type: String,
    #[serde(default)]
    pub default_value: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<ConfigOption>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Manifest {
    pub api_version: u32,
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
    pub license: String,
    pub repository: String,
    pub permissions: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_abi: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_server_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_server_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checksum: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub banner: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_schema: Option<Vec<ConfigField>>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Package {
    pub manifest: Manifest,
    pub publisher_id: String,
    pub wasm: String,
    pub sha256: String,
    pub signature: String,
}

impl Package {
    /// Exact signing input: compact serde JSON tuple, in this field order.
    pub fn signing_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(&(
            "media-extension-package-v1",
            &self.manifest,
            &self.publisher_id,
            &self.sha256,
        ))
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MetadataInput {
    pub api_version: u32,
    pub work_id: String,
    pub library_id: String,
    pub title: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MetadataProposal {
    pub title: String,
    pub summary: String,
}

/// Implement the ABI without exposing raw pointers to extension authors.
/// Buffers live for one invocation; the host destroys the whole guest afterwards.
#[macro_export]
macro_rules! extension {
    ($handler:path) => {
        #[cfg(target_arch = "wasm32")]
        #[unsafe(no_mangle)]
        pub extern "C" fn media_alloc(len: u32) -> u32 {
            if len as usize > $crate::MAX_JSON {
                return 0;
            }
            let buffer = vec![0u8; len as usize].into_boxed_slice();
            Box::into_raw(buffer) as *mut u8 as u32
        }
        #[cfg(target_arch = "wasm32")]
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn media_run(ptr: u32, len: u32) -> u64 {
            if len as usize > $crate::MAX_JSON {
                return 0;
            }
            // The ABI host supplies the allocation returned by media_alloc.
            let bytes = unsafe { std::slice::from_raw_parts(ptr as *const u8, len as usize) };
            let Ok(input) = $crate::serde_json::from_slice::<$crate::ExtensionRequest>(bytes)
            else {
                return 0;
            };
            let result: $crate::ExtensionResponse = $handler(input);
            let Ok(bytes) = $crate::serde_json::to_vec(&result) else {
                return 0;
            };
            if bytes.len() > $crate::MAX_JSON {
                return 0;
            }
            let len = bytes.len() as u64;
            let ptr = Box::into_raw(bytes.into_boxed_slice()) as *mut u8 as u32;
            ((ptr as u64) << 32) | len
        }
    };
}
#[doc(hidden)]
pub use serde_json;
