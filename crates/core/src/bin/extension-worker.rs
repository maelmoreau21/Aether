fn main() {
    use aether_core::extensions::runtime::{execute_brokered, last_trap, read_frame, write_frame};
    // Release the initial input lock before guest host-calls read broker replies.
    let invocation = {
        let input = std::io::stdin();
        let mut input = input.lock();
        // The module (base64) and the largest request, plus hosts and headers.
        read_frame(
            &mut input,
            media_extension_sdk::MAX_MODULE * 4 / 3 + media_extension_sdk::MAX_INPUT_JSON + 65_536,
        )
    };
    let result = invocation.and_then(execute_brokered);
    match result {
        Ok(response) => {
            let _ = write_frame(
                &mut std::io::stdout().lock(),
                &serde_json::json!({"kind":"finished","response":response}),
            );
        }
        Err(code) => {
            let _ = write_frame(
                &mut std::io::stdout().lock(),
                &serde_json::json!({"kind":"failed","error":code,"trap":last_trap()}),
            );
            std::process::exit(1);
        }
    }
}
