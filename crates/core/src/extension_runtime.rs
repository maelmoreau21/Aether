//! Import-controlled Wasm execution with brokered HTTP capability and quotas.
//! Only invoked in the disposable worker process.

use base64::{Engine as _, engine::general_purpose::STANDARD};
use media_extension_sdk::{
    ExtensionRequest, ExtensionResponse, HostHttpError, HostHttpRequest, HostHttpResponse,
    MAX_JSON, MAX_MODULE,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::net::IpAddr;
use std::time::Duration;
use wasmi::{
    Caller, Config, EnforcedLimits, Engine, Extern, Linker, Module, Store, StoreLimits,
    StoreLimitsBuilder,
};

pub const DEFAULT_MAX_HTTP_REQUESTS: usize = 10;
pub const DEFAULT_MAX_RESPONSE_SIZE: usize = 262_144; // 256 KiB
pub const DEFAULT_HTTP_TIMEOUT_SECS: u64 = 6;
// Ten bounded HTTP requests plus compilation/serialization, below the Web request deadline.
pub const INVOCATION_TIMEOUT_SECS: u64 = 65;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Invocation {
    pub wasm: String,
    pub input: ExtensionRequest,
    #[serde(default)]
    pub allowed_hosts: Vec<String>,
    #[serde(default)]
    pub auth_headers: BTreeMap<String, String>,
}

impl Invocation {
    pub fn new(wasm: String, input: ExtensionRequest) -> Self {
        Self {
            wasm,
            input,
            allowed_hosts: Vec::new(),
            auth_headers: BTreeMap::new(),
        }
    }
}

pub struct HostState {
    pub limits: StoreLimits,
    pub allowed_hosts: Vec<String>,
    pub auth_headers: BTreeMap<String, String>,
    pub http_count: usize,
    pub max_http_requests: usize,
    pub max_response_size: usize,
}

pub(crate) fn is_forbidden_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_documentation()
                || v4.is_unspecified()
                || v4.is_multicast()
                || v4.octets()[0] == 0
                || v4.octets()[0] >= 240
                || (v4.octets()[0] == 100 && (64..=127).contains(&v4.octets()[1]))
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_unique_local()
                || v6.is_unicast_link_local()
                || v6.is_multicast()
                || v6
                    .to_ipv4_mapped()
                    .is_some_and(|v4| is_forbidden_ip(IpAddr::V4(v4)))
        }
    }
}

pub fn execute_host_http(
    state: &mut HostState,
    req: HostHttpRequest,
) -> Result<HostHttpResponse, HostHttpError> {
    if state.http_count >= state.max_http_requests {
        return Err(HostHttpError {
            code: "quota_exceeded".into(),
            message: format!(
                "Maximum {} HTTP requests per invocation exceeded",
                state.max_http_requests
            ),
        });
    }
    state.http_count += 1;

    let parsed_url = url::Url::parse(&req.url).map_err(|e| HostHttpError {
        code: "invalid_url".into(),
        message: e.to_string(),
    })?;

    if parsed_url.scheme() != "https" {
        return Err(HostHttpError {
            code: "scheme_forbidden".into(),
            message: "Only HTTPS URLs are permitted".into(),
        });
    }

    let host = parsed_url.host_str().ok_or_else(|| HostHttpError {
        code: "missing_host".into(),
        message: "URL lacks a valid hostname".into(),
    })?;

    // Check SSRF protection against private IP / loopback
    if let Ok(ip) = host.parse::<IpAddr>()
        && is_forbidden_ip(ip)
    {
        return Err(HostHttpError {
            code: "host_forbidden".into(),
            message: "Private IP and loopback addresses are forbidden".into(),
        });
    }
    if host.eq_ignore_ascii_case("localhost") {
        return Err(HostHttpError {
            code: "host_forbidden".into(),
            message: "Localhost access is forbidden".into(),
        });
    }

    // Check allowlist
    let is_allowed = state
        .allowed_hosts
        .iter()
        .any(|allowed| host.eq_ignore_ascii_case(allowed));
    if !is_allowed {
        return Err(HostHttpError {
            code: "domain_not_allowed".into(),
            message: format!(
                "Host '{}' is not in allowed domains for this extension",
                host
            ),
        });
    }

    if !parsed_url.username().is_empty()
        || parsed_url.password().is_some()
        || parsed_url.port_or_known_default() != Some(443)
    {
        return Err(HostHttpError {
            code: "invalid_url".into(),
            message: "HTTPS on port 443 without URL credentials is required".into(),
        });
    }
    let host = host.to_owned();
    let timeout_secs = req
        .timeout_secs
        .map(|s| (s as u64).min(DEFAULT_HTTP_TIMEOUT_SECS))
        .unwrap_or(DEFAULT_HTTP_TIMEOUT_SECS);

    // Perform request synchronously using a lightweight tokio runtime handle
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| HostHttpError {
            code: "runtime_error".into(),
            message: e.to_string(),
        })?;

    let max_resp = state.max_response_size;
    let auth_headers = state.auth_headers.clone();

    rt.block_on(async move {
        let addresses: Vec<_> = tokio::time::timeout(
            Duration::from_secs(timeout_secs),
            tokio::net::lookup_host((host.as_str(), 443)),
        )
        .await
        .map_err(|_| HostHttpError {
            code: "dns_timeout".into(),
            message: "DNS lookup timed out".into(),
        })?
        .map_err(|_| HostHttpError {
            code: "dns_error".into(),
            message: "DNS lookup failed".into(),
        })?
        .collect();
        if addresses.is_empty() || addresses.iter().any(|a| is_forbidden_ip(a.ip())) {
            return Err(HostHttpError {
                code: "host_forbidden".into(),
                message: "Non-public address denied".into(),
            });
        }
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .resolve_to_addrs(&host, &addresses)
            .timeout(Duration::from_secs(timeout_secs))
            .build()
            .map_err(|e| HostHttpError {
                code: "client_error".into(),
                message: e.to_string(),
            })?;

        let mut builder = match req.method.to_uppercase().as_str() {
            "GET" => client.get(parsed_url),
            "POST" => client.post(parsed_url),
            "PUT" => client.put(parsed_url),
            "DELETE" => client.delete(parsed_url),
            _ => {
                return Err(HostHttpError {
                    code: "method_unsupported".into(),
                    message: format!("Method {} not supported", req.method),
                });
            }
        };

        // Add user-supplied headers
        for (k, v) in &req.headers {
            builder = builder.header(k, v);
        }

        // Inject host-protected authentication headers
        for (k, v) in auth_headers {
            builder = builder.header(k, v);
        }

        if let Some(body) = req.body {
            builder = builder.body(body);
        }

        let mut response = builder.send().await.map_err(|e| HostHttpError {
            code: "network_error".into(),
            message: e.to_string(),
        })?;

        let status = response.status().as_u16();
        let mut resp_headers = BTreeMap::new();
        for (k, v) in response.headers() {
            if let Ok(v_str) = v.to_str() {
                resp_headers.insert(k.as_str().to_string(), v_str.to_string());
            }
        }

        let mut body_bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| HostHttpError {
            code: "read_error".into(),
            message: "Response read failed".into(),
        })? {
            if body_bytes.len() + chunk.len() > max_resp {
                return Err(HostHttpError {
                    code: "response_too_large".into(),
                    message: "Response exceeds quota".into(),
                });
            }
            body_bytes.extend_from_slice(&chunk);
        }

        Ok(HostHttpResponse {
            status,
            headers: resp_headers,
            body: body_bytes.to_vec(),
        })
    })
}

fn write_host_result_to_guest<T: Serialize>(caller: &mut Caller<'_, HostState>, result: T) -> u64 {
    let Ok(bytes) = serde_json::to_vec(&result) else {
        return 0;
    };
    // HTTP bodies serialize as JSON byte arrays (up to four bytes per input byte).
    if bytes.len() > DEFAULT_MAX_RESPONSE_SIZE * 4 + 8192 {
        return 0;
    }
    let Some(Extern::Func(alloc_func)) = caller.get_export("media_alloc") else {
        return 0;
    };
    let Ok(alloc_typed) = alloc_func.typed::<u32, u32>(&caller) else {
        return 0;
    };
    let Ok(ptr) = alloc_typed.call(&mut *caller, bytes.len() as u32) else {
        return 0;
    };
    let Some(Extern::Memory(memory)) = caller.get_export("memory") else {
        return 0;
    };
    if memory.write(&mut *caller, ptr as usize, &bytes).is_err() {
        return 0;
    }
    ((ptr as u64) << 32) | (bytes.len() as u64)
}

pub fn execute(invocation: Invocation) -> Result<ExtensionResponse, &'static str> {
    execute_inner(invocation, false)
}

/// Only the disposable worker enables its private, framed Core pipe.
pub fn execute_brokered(invocation: Invocation) -> Result<ExtensionResponse, &'static str> {
    execute_inner(invocation, true)
}

fn execute_inner(
    invocation: Invocation,
    brokered: bool,
) -> Result<ExtensionResponse, &'static str> {
    if invocation.input.api_version != 1
        || invocation.input.operation.is_empty()
        || invocation.input.operation.len() > 80
    {
        return Err("invalid_input");
    }
    if invocation.wasm.len() > MAX_MODULE * 4 / 3 + 4 {
        return Err("module_too_large");
    }
    let bytes = STANDARD
        .decode(invocation.wasm)
        .map_err(|_| "invalid_module")?;
    if bytes.len() > MAX_MODULE || !bytes.starts_with(b"\0asm") {
        return Err("invalid_module");
    }
    let mut config = Config::default();
    config.consume_fuel(true).allow_start_fn(false);
    config.enforced_limits(EnforcedLimits::strict());
    // Wasmi measures the value stack in bytes, independently of recursion depth.
    // 1 KiB rejected ordinary Rust/serde modules before their handler could run.
    config.set_max_stack_height(64 * 1024);
    config.set_max_recursion_depth(128);
    let engine = Engine::new(&config);
    let module = Module::new(&engine, &bytes).map_err(|_| "invalid_module")?;

    // Explicit HTTP and versioned storage imports only; no WASI, SQL or filesystem.
    for import in module.imports() {
        if !(import.module() == "aether" && import.name() == "aether_host_http"
            || import.module() == "media"
                && matches!(
                    import.name(),
                    "media_host_storage_v1" | "media_host_objects_v1"
                ))
        {
            return Err("imports_forbidden");
        }
    }

    let limits = StoreLimitsBuilder::new()
        .memory_size(16 * 1024 * 1024)
        .memories(1)
        .tables(1)
        .table_elements(4096)
        .instances(1)
        .build();

    let host_state = HostState {
        limits,
        allowed_hosts: invocation.allowed_hosts,
        auth_headers: invocation.auth_headers,
        http_count: 0,
        max_http_requests: DEFAULT_MAX_HTTP_REQUESTS,
        max_response_size: DEFAULT_MAX_RESPONSE_SIZE,
    };

    let mut store = Store::new(&engine, host_state);
    store.limiter(|state| &mut state.limits);
    store.set_fuel(2_000_000).map_err(|_| "runtime_error")?;

    let mut linker = Linker::new(&engine);
    let _ = linker.func_wrap(
        "aether",
        "aether_host_http",
        |mut caller: Caller<'_, HostState>, ptr: u32, len: u32| -> u64 {
            if len as usize > MAX_JSON {
                let err_res: Result<HostHttpResponse, HostHttpError> = Err(HostHttpError {
                    code: "request_too_large".into(),
                    message: "Request size exceeds limit".into(),
                });
                return write_host_result_to_guest(&mut caller, err_res);
            }

            let Some(Extern::Memory(memory)) = caller.get_export("memory") else {
                return 0;
            };

            let mut req_bytes = vec![0u8; len as usize];
            if memory.read(&caller, ptr as usize, &mut req_bytes).is_err() {
                return 0;
            }

            let parsed_req: Result<HostHttpRequest, _> = serde_json::from_slice(&req_bytes);
            let result = match parsed_req {
                Ok(req) => execute_host_http(caller.data_mut(), req),
                Err(e) => Err(HostHttpError {
                    code: "invalid_json".into(),
                    message: e.to_string(),
                }),
            };

            write_host_result_to_guest(&mut caller, result)
        },
    );

    for (name, kind) in [
        ("media_host_storage_v1", "storage"),
        ("media_host_objects_v1", "objects"),
    ] {
        linker
            .func_wrap(
                "media",
                name,
                move |mut caller: Caller<'_, HostState>, ptr: u32, len: u32| -> u64 {
                    let fail = |code: &str| serde_json::json!({"Err":{"code":code,"message":code}});
                    if !brokered {
                        return write_host_result_to_guest(
                            &mut caller,
                            fail("storage_unavailable"),
                        );
                    }
                    if len as usize > MAX_JSON {
                        return write_host_result_to_guest(&mut caller, fail("request_too_large"));
                    }
                    let Some(Extern::Memory(memory)) = caller.get_export("memory") else {
                        return 0;
                    };
                    let mut bytes = vec![0; len as usize];
                    if memory.read(&caller, ptr as usize, &mut bytes).is_err() {
                        return 0;
                    }
                    let Ok(request) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
                        return write_host_result_to_guest(&mut caller, fail("invalid_request"));
                    };
                    let result = (|| {
                        write_frame(
                            &mut std::io::stdout().lock(),
                            &serde_json::json!({"kind":kind,"request":request}),
                        )?;
                        read_frame::<serde_json::Value>(&mut std::io::stdin().lock(), MAX_JSON)
                    })()
                    .unwrap_or_else(|_| fail("broker_unavailable"));
                    write_host_result_to_guest(&mut caller, result)
                },
            )
            .map_err(|_| "runtime_error")?;
    }

    let instance = linker
        .instantiate_and_start(&mut store, &module)
        .map_err(|_| "instantiation_failed")?;
    let memory = instance
        .get_memory(&store, "memory")
        .ok_or("missing_memory")?;
    let alloc = instance
        .get_typed_func::<u32, u32>(&store, "media_alloc")
        .map_err(|_| "invalid_abi")?;
    let run = instance
        .get_typed_func::<(u32, u32), u64>(&store, "media_run")
        .map_err(|_| "invalid_abi")?;
    let input = serde_json::to_vec(&invocation.input).map_err(|_| "invalid_input")?;
    if input.len() > MAX_JSON {
        return Err("input_too_large");
    }
    let ptr = alloc
        .call(&mut store, input.len() as u32)
        .map_err(|_| "execution_trapped")?;
    memory
        .write(&mut store, ptr as usize, &input)
        .map_err(|_| "invalid_pointer")?;
    let packed = run
        .call(&mut store, (ptr, input.len() as u32))
        .map_err(|_| "execution_trapped")?;
    let length = (packed & 0xffff_ffff) as usize;
    if length == 0 || length > MAX_JSON {
        return Err("invalid_output_size");
    }
    let mut output = vec![0; length];
    memory
        .read(&store, (packed >> 32) as usize, &mut output)
        .map_err(|_| "invalid_pointer")?;
    let response: ExtensionResponse =
        serde_json::from_slice(&output).map_err(|_| "invalid_output")?;
    if response.api_version != 1 {
        return Err("invalid_output");
    }
    Ok(response)
}

/// Length prefix bounds allocation before JSON parsing. No guest data is logged.
pub fn read_frame<T: serde::de::DeserializeOwned>(
    reader: &mut impl std::io::Read,
    maximum: usize,
) -> Result<T, &'static str> {
    let mut header = [0; 4];
    reader.read_exact(&mut header).map_err(|_| "frame_io")?;
    let len = u32::from_be_bytes(header) as usize;
    if len > maximum {
        return Err("frame_too_large");
    }
    let mut bytes = vec![0; len];
    reader.read_exact(&mut bytes).map_err(|_| "frame_io")?;
    serde_json::from_slice(&bytes).map_err(|_| "frame_json")
}
pub fn write_frame(
    writer: &mut impl std::io::Write,
    value: &impl Serialize,
) -> Result<(), &'static str> {
    let bytes = serde_json::to_vec(value).map_err(|_| "frame_json")?;
    let len = u32::try_from(bytes.len()).map_err(|_| "frame_too_large")?;
    writer
        .write_all(&len.to_be_bytes())
        .map_err(|_| "frame_io")?;
    writer.write_all(&bytes).map_err(|_| "frame_io")?;
    writer.flush().map_err(|_| "frame_io")
}

#[cfg(test)]
mod storage_frames {
    use super::*;
    #[test]
    fn broker_rejects_oversized_and_truncated_frames_before_parsing() {
        let header = (MAX_JSON as u32 + 1).to_be_bytes();
        assert_eq!(
            read_frame::<serde_json::Value>(&mut &header[..], MAX_JSON),
            Err("frame_too_large")
        );
        assert_eq!(
            read_frame::<serde_json::Value>(&mut &[0, 0, 0, 3, b'{'][..], MAX_JSON),
            Err("frame_io")
        );
        let mut bytes = Vec::new();
        write_frame(&mut bytes, &serde_json::json!({"kind":"storage"})).unwrap();
        assert_eq!(
            read_frame::<serde_json::Value>(&mut &bytes[..], MAX_JSON).unwrap()["kind"],
            "storage"
        );
    }
}
