fn main() {
    use media_poc::extension_runtime::{execute_brokered, read_frame, write_frame};
    // Release the initial input lock before guest host-calls read broker replies.
    let invocation = {
        let input = std::io::stdin();
        let mut input = input.lock();
        read_frame(&mut input, 400_000)
    };
    let result = invocation.and_then(execute_brokered);
    match result {
        Ok(response) => {
            let _ = write_frame(
                &mut std::io::stdout().lock(),
                &serde_json::json!({"kind":"finished","response":response}),
            );
        }
        Err(_) => std::process::exit(1),
    }
}
