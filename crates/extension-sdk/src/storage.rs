//! Versioned private storage. Identity and authorization are supplied by the Core.
use serde::{Deserialize, Serialize};

pub const STORAGE_API_VERSION: u32 = 1;
pub const MAX_STORAGE_VALUE: usize = 16 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StorageScope {
    pub kind: String,
    /// Empty selects the current host-bound resource. Explicit IDs must match it.
    pub id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StorageRequest {
    pub api_version: u32,
    pub scope: StorageScope,
    pub operation: StorageOperation,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum StorageOperation {
    Get {
        key: String,
    },
    Put {
        key: String,
        value: String,
        expected_version: Option<String>,
        expires_at: Option<i64>,
        durability: String,
    },
    Delete {
        key: String,
        expected_version: Option<String>,
    },
    List {
        after: String,
        limit: u32,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StorageEntry {
    pub key: String,
    pub value: String,
    pub version: String,
    pub expires_at: Option<i64>,
    pub durability: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StorageResponse {
    pub entries: Vec<StorageEntry>,
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StorageError {
    pub code: String,
    pub message: String,
}

#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "media")]
unsafe extern "C" {
    fn media_host_storage_v1(ptr: u32, len: u32) -> u64;
}

pub fn host_storage(request: &StorageRequest) -> Result<StorageResponse, StorageError> {
    let bytes = serde_json::to_vec(request).map_err(|_| error("invalid_request"))?;
    if bytes.len() > crate::MAX_JSON {
        return Err(error("request_too_large"));
    }
    #[cfg(target_arch = "wasm32")]
    {
        let packed = unsafe { media_host_storage_v1(bytes.as_ptr() as u32, bytes.len() as u32) };
        let len = (packed & 0xffff_ffff) as usize;
        if packed == 0 || len > crate::MAX_JSON {
            return Err(error("invalid_host_response"));
        }
        let bytes = unsafe { std::slice::from_raw_parts((packed >> 32) as *const u8, len) };
        serde_json::from_slice(bytes).map_err(|_| error("invalid_host_response"))?
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        Err(error("unsupported_platform"))
    }
}

fn error(code: &str) -> StorageError {
    StorageError {
        code: code.into(),
        message: code.into(),
    }
}
