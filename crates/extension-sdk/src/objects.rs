//! Chunked private objects. References are opaque, scoped and never filesystem paths.
use crate::storage::StorageScope;
use serde::{Deserialize, Serialize};

pub const OBJECT_API_VERSION: u32 = 1;
pub const OBJECT_CHUNK_BYTES: usize = 16 * 1024;
pub const MAX_OBJECT_BYTES: i64 = 16 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ObjectRequest {
    pub api_version: u32,
    pub scope: StorageScope,
    pub operation: ObjectOperation,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum ObjectOperation {
    Begin {
        size: i64,
        sha256: String,
        durability: String,
        expires_at: Option<i64>,
    },
    Append {
        id: String,
        offset: i64,
        data_base64: String,
    },
    Finish {
        id: String,
    },
    Read {
        id: String,
        offset: i64,
    },
    Delete {
        id: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ObjectMetadata {
    pub id: String,
    pub size: i64,
    pub sha256: String,
    pub durability: String,
    pub expires_at: Option<i64>,
    pub complete: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ObjectResponse {
    pub metadata: Option<ObjectMetadata>,
    pub data_base64: Option<String>,
    pub next_offset: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ObjectError {
    pub code: String,
    pub message: String,
}

#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "media")]
unsafe extern "C" {
    fn media_host_objects_v1(ptr: u32, len: u32) -> u64;
}

pub fn host_objects(request: &ObjectRequest) -> Result<ObjectResponse, ObjectError> {
    let bytes = serde_json::to_vec(request).map_err(|_| error("invalid_request"))?;
    if bytes.len() > crate::MAX_JSON {
        return Err(error("request_too_large"));
    }
    #[cfg(target_arch = "wasm32")]
    {
        let packed = unsafe { media_host_objects_v1(bytes.as_ptr() as u32, bytes.len() as u32) };
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

fn error(code: &str) -> ObjectError {
    ObjectError {
        code: code.into(),
        message: code.into(),
    }
}
