//! Import-free Wasm execution. Only invoked in the disposable worker process.
use base64::{Engine as _, engine::general_purpose::STANDARD};
use media_extension_sdk::{ExtensionRequest, ExtensionResponse, MAX_JSON, MAX_MODULE};
use serde::{Deserialize, Serialize};
use wasmi::{Config, EnforcedLimits, Engine, Linker, Module, Store, StoreLimitsBuilder};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Invocation {
    pub wasm: String,
    pub input: ExtensionRequest,
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
    if module.imports().next().is_some() {
        return Err("imports_forbidden");
    }
    let limits = StoreLimitsBuilder::new()
        .memory_size(16 * 1024 * 1024)
        .memories(1)
        .tables(1)
        .table_elements(4096)
        .instances(1)
        .build();
    let mut store = Store::new(&engine, limits);
    store.limiter(|limits| limits);
    store.set_fuel(2_000_000).map_err(|_| "runtime_error")?;
    let instance = Linker::new(&engine)
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
