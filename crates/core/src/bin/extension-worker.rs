use std::io::{Read, Write};

fn main() {
    // Never print guest data or internal diagnostics on failure.
    let mut bytes = Vec::new();
    let result = std::io::stdin()
        .take(400_001)
        .read_to_end(&mut bytes)
        .map_err(|_| "input_error")
        .and_then(|_| {
            if bytes.len() > 400_000 {
                return Err("input_too_large");
            }
            serde_json::from_slice(&bytes).map_err(|_| "invalid_input")
        })
        .and_then(media_poc::extension_runtime::execute);
    match result {
        Ok(result) => {
            if let Ok(bytes) = serde_json::to_vec(&result) {
                let _ = std::io::stdout().write_all(&bytes);
            }
        }
        Err(_) => std::process::exit(1),
    }
}
