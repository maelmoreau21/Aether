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

fn is_forbidden_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_documentation()
                || v4.is_unspecified()
        }
        IpAddr::V6(v6) => v6.is_loopback() || v6.is_unspecified(),
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
        let client = reqwest::Client::builder()
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

        let response = builder.send().await.map_err(|e| HostHttpError {
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

        let body_bytes = response.bytes().await.map_err(|e| HostHttpError {
            code: "read_error".into(),
            message: e.to_string(),
        })?;

        if body_bytes.len() > max_resp {
            return Err(HostHttpError {
                code: "response_too_large".into(),
                message: format!(
                    "Response body size {} exceeds limit {}",
                    body_bytes.len(),
                    max_resp
                ),
            });
        }

        Ok(HostHttpResponse {
            status,
            headers: resp_headers,
            body: body_bytes.to_vec(),
        })
    })
}

fn write_host_result_to_guest(
    caller: &mut Caller<'_, HostState>,
    result: Result<HostHttpResponse, HostHttpError>,
) -> u64 {
    let Ok(bytes) = serde_json::to_vec(&result) else {
        return 0;
    };
    if bytes.len() > MAX_JSON {
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
    config.set_max_stack_height(1024);
    let engine = Engine::new(&config);
    let module = Module::new(&engine, &bytes).map_err(|_| "invalid_module")?;

    // Strictly enforce imports: only allow "aether" / "aether_host_http"
    for import in module.imports() {
        if import.module() != "aether" || import.name() != "aether_host_http" {
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
                let err_res = Err(HostHttpError {
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
